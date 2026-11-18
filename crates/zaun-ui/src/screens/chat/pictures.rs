//! Profile pictures as textures (ADR-0038; chat-calls.html C "cache"). One worker thread
//! decodes them, never the UI thread (PERFORMANCE §1), as `media.rs` and `gif.rs` do. An avatar
//! is centre-cropped and shrunk to [`SIDE`] there, then uploaded here once per
//! `Picture::key()`. The crop step's whole picture has a slot of its own.

use std::collections::HashMap;
use std::io::Cursor;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};

use egui::epaint::Brush;
use egui::load::{ImageLoadResult, ImageLoader, LoadError, SizeHint};
use egui::{ColorImage, Context, Id, Rect, TextureHandle, TextureId, TextureOptions, pos2};
use zaun_core::model::Picture;

/// Avatars are uploaded this many pixels a side: the largest drawn (96) at 2×.
const SIDE: u32 = 192;
/// The crop step's picture is shrunk to at most this many pixels a side.
const WHOLE_SIDE: u32 = 1024;
/// Texture memory the avatars keep at most.
const BUDGET: usize = 8 * 1024 * 1024;
/// Avatars kept at most, counting the ones that failed (they hold no texture).
const MAX_ENTRIES: usize = 256;
/// Jobs waiting for the worker at most. One that does not fit is asked again next frame.
const QUEUE: usize = 32;

/// Linear filtering with mipmaps: a 192 px picture drawn at 22 stays smooth.
const SMOOTH: TextureOptions = TextureOptions {
    mipmap_mode: Some(egui::TextureFilter::Linear),
    ..TextureOptions::LINEAR
};

/// What became of a picture asked for.
enum Slot<T> {
    Loading,
    /// Uploaded at UI time `since` (s), `bytes` of texture memory.
    Ready {
        tex: T,
        bytes: usize,
        since: f64,
    },
    Failed,
}

/// The avatars by key, least recently drawn first out.
///
/// EVICTION: over [`BUDGET`] bytes or [`MAX_ENTRIES`] entries, the entries drawn longest ago
/// go first. Pictures still decoding and pictures drawn in this pass are never dropped. A
/// friend's new picture has a new key, so their old one ages out the same way.
struct Lru<T> {
    entries: HashMap<u64, (Slot<T>, u64)>,
    bytes: usize,
}

impl<T> Default for Lru<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
        }
    }
}

impl<T> Lru<T> {
    /// The slot of `key`, marked as drawn in `pass`.
    fn touch(&mut self, key: u64, pass: u64) -> Option<&Slot<T>> {
        let (slot, used) = self.entries.get_mut(&key)?;
        *used = pass;
        Some(slot)
    }

    fn put(&mut self, key: u64, slot: Slot<T>, pass: u64) {
        if let Slot::Ready { bytes, .. } = &slot {
            self.bytes += bytes;
        }
        if let Some((old, _)) = self.entries.insert(key, (slot, pass)) {
            self.forget(&old);
        }
    }

    fn forget(&mut self, slot: &Slot<T>) {
        if let Slot::Ready { bytes, .. } = slot {
            self.bytes -= bytes;
        }
    }

    #[cfg(test)]
    fn bytes(&self) -> usize {
        self.bytes
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn loading(&self) -> usize {
        self.entries
            .values()
            .filter(|(s, _)| matches!(s, Slot::Loading))
            .count()
    }

    /// Drop the least recently drawn entries until the cache is within its limits.
    fn evict(&mut self, pass: u64) {
        if self.bytes <= BUDGET && self.entries.len() <= MAX_ENTRIES {
            return;
        }
        let mut by_age: Vec<(u64, u64)> = self
            .entries
            .iter()
            .filter(|(_, (s, used))| !matches!(s, Slot::Loading) && *used < pass)
            .map(|(k, (_, used))| (*used, *k))
            .collect();
        by_age.sort_unstable();
        for (_, key) in by_age {
            if self.bytes <= BUDGET && self.entries.len() <= MAX_ENTRIES {
                break;
            }
            if let Some((old, _)) = self.entries.remove(&key) {
                self.forget(&old);
            }
        }
    }
}

/// A drawable avatar: its texture as a brush (shared, so drawing allocates nothing).
#[derive(Clone)]
struct Face {
    _tex: TextureHandle,
    brush: Arc<Brush>,
}

/// The crop step's picture.
#[derive(Clone)]
struct Whole {
    _tex: TextureHandle,
    id: TextureId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Avatar,
    Whole,
}

type Decoded = Result<([usize; 2], Vec<u8>), String>;
type Job = (u64, Kind, Arc<[u8]>);
type Done = (u64, Kind, Decoded);

struct Cache {
    faces: Lru<Face>,
    /// One at a time: asking for another replaces it, and it goes once not drawn for a pass.
    whole: Option<(u64, Slot<Whole>, u64)>,
    jobs: Option<mpsc::SyncSender<Job>>,
    done_tx: mpsc::Sender<Done>,
    /// At most one result per entry still loading (bounded by [`MAX_ENTRIES`] + 1).
    done: mpsc::Receiver<Done>,
    /// Pictures still decoding, read by kittest through [`Pending`].
    pending: Arc<AtomicUsize>,
}

type Shared = Arc<Mutex<Cache>>;

fn cache(ctx: &Context) -> Shared {
    if let Some(c) = ctx.data(|d| d.get_temp::<Shared>(Id::unique("chat.pictures"))) {
        return c;
    }
    let (done_tx, done) = mpsc::channel();
    let pending = Arc::new(AtomicUsize::new(0));
    let shared: Shared = Arc::new(Mutex::new(Cache {
        faces: Lru::default(),
        whole: None,
        jobs: None,
        done_tx,
        done,
        pending: Arc::clone(&pending),
    }));
    ctx.add_image_loader(Arc::new(Pending(pending)));
    ctx.data_mut(|d| d.insert_temp(Id::unique("chat.pictures"), Arc::clone(&shared)));
    shared
}

/// Tells egui (and so kittest's `run`) that pictures are still decoding. It loads nothing.
struct Pending(Arc<AtomicUsize>);

impl ImageLoader for Pending {
    fn id(&self) -> &str {
        "zaun.chat.pictures"
    }
    fn load(&self, _: &Context, _: &str, _: SizeHint) -> ImageLoadResult {
        Err(LoadError::NotSupported)
    }
    fn forget(&self, _: &str) {}
    fn forget_all(&self) {}
    fn byte_size(&self) -> usize {
        0
    }
    fn has_pending(&self) -> bool {
        self.0.load(Ordering::Relaxed) > 0
    }
}

/// Decode `bytes` for `kind`: an avatar centre-cropped to a square of at most [`SIDE`], the
/// crop step's picture at most [`WHOLE_SIDE`] a side.
fn decode(bytes: &[u8], kind: Kind) -> Decoded {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    let img = match kind {
        Kind::Avatar => {
            let s = w.min(h);
            let square = img.crop_imm((w - s) / 2, (h - s) / 2, s, s);
            if s > SIDE {
                square.resize_exact(SIDE, SIDE, image::imageops::FilterType::CatmullRom)
            } else {
                square
            }
        }
        Kind::Whole if w.max(h) > WHOLE_SIDE => img.resize(
            WHOLE_SIDE,
            WHOLE_SIDE,
            image::imageops::FilterType::Triangle,
        ),
        Kind::Whole => img,
    };
    let rgba = img.to_rgba8();
    Ok((
        [rgba.width() as usize, rgba.height() as usize],
        rgba.into_raw(),
    ))
}

fn worker(ctx: Context, jobs: mpsc::Receiver<Job>, done: mpsc::Sender<Done>) {
    for (key, kind, bytes) in jobs {
        // A friend sends these bytes: a decoder that panics costs the picture, not the thread.
        let out = std::panic::catch_unwind(AssertUnwindSafe(|| decode(&bytes, kind)))
            .unwrap_or_else(|_| Err("decoder panicked".to_owned()));
        if done.send((key, kind, out)).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

impl Cache {
    /// Send a job to the worker (started the first time). `false`: it cannot take it now.
    fn ask(&mut self, ctx: &Context, key: u64, kind: Kind, bytes: Arc<[u8]>) -> Option<bool> {
        if self.jobs.is_none() {
            let (tx, rx) = mpsc::sync_channel(QUEUE);
            let (c, done) = (ctx.clone(), self.done_tx.clone());
            std::thread::Builder::new()
                .name("chat-picture-decode".into())
                .spawn(move || worker(c, rx, done))
                .ok()?;
            self.jobs = Some(tx);
        }
        match self.jobs.as_ref()?.try_send((key, kind, bytes)) {
            Ok(()) => Some(true),
            Err(mpsc::TrySendError::Full(_)) => Some(false),
            Err(mpsc::TrySendError::Disconnected(_)) => None,
        }
    }

    /// Upload what the worker finished, then keep within the limits.
    fn collect(&mut self, ctx: &Context, pass: u64) {
        let now = ctx.input(|i| i.time);
        while let Ok((key, kind, out)) = self.done.try_recv() {
            let slot = out
                .inspect_err(|e| tracing::debug!(error = %e, "chat_picture_undecodable"))
                .map(|(size, rgba)| {
                    let img = ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let name = format!("chat-picture-{key:016x}");
                    (ctx.load_texture(name, img, SMOOTH), rgba.len())
                });
            match kind {
                Kind::Avatar => {
                    let slot = match slot {
                        Ok((tex, bytes)) => Slot::Ready {
                            tex: face(tex),
                            bytes,
                            since: now,
                        },
                        Err(_) => Slot::Failed,
                    };
                    self.faces.put(key, slot, pass);
                }
                Kind::Whole => {
                    if let Some((k, s, _)) = &mut self.whole
                        && *k == key
                    {
                        *s = match slot {
                            Ok((tex, bytes)) => Slot::Ready {
                                tex: Whole {
                                    id: tex.id(),
                                    _tex: tex,
                                },
                                bytes,
                                since: now,
                            },
                            Err(_) => Slot::Failed,
                        };
                    }
                }
            }
        }
        self.faces.evict(pass);
        if self.whole.as_ref().is_some_and(|(_, s, used)| {
            !matches!(s, Slot::Loading) && used.saturating_add(1) < pass
        }) {
            self.whole = None;
        }
        let whole_loading = matches!(self.whole, Some((_, Slot::Loading, _)));
        self.pending.store(
            self.faces.loading() + usize::from(whole_loading),
            Ordering::Relaxed,
        );
    }
}

fn face(tex: TextureHandle) -> Face {
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    Face {
        brush: Arc::new(Brush {
            fill_texture_id: tex.id(),
            uv: full,
        }),
        _tex: tex,
    }
}

/// The avatar texture of `picture` and when it was uploaded (UI time, s); asks for it the
/// first time. `None` while it decodes or when it cannot be decoded.
pub fn avatar(ctx: &Context, picture: &Picture) -> Option<(Arc<Brush>, f64)> {
    let shared = cache(ctx);
    let mut c = shared.lock().unwrap_or_else(PoisonError::into_inner);
    let pass = ctx.cumulative_pass_nr();
    c.collect(ctx, pass);
    let key = picture.key();
    match c.faces.touch(key, pass) {
        Some(Slot::Ready { tex, since, .. }) => return Some((Arc::clone(&tex.brush), *since)),
        Some(Slot::Loading | Slot::Failed) => return None,
        None => {}
    }
    match c.ask(ctx, key, Kind::Avatar, picture.shared()) {
        Some(true) => c.faces.put(key, Slot::Loading, pass),
        Some(false) => ctx.request_repaint(),
        None => c.faces.put(key, Slot::Failed, pass),
    }
    c.pending.store(c.faces.loading(), Ordering::Relaxed);
    None
}

/// The crop step's whole picture (at most 1024 px a side); asks for it the first time.
pub fn whole(ctx: &Context, picture: &Picture) -> Option<TextureId> {
    let shared = cache(ctx);
    let mut c = shared.lock().unwrap_or_else(PoisonError::into_inner);
    let pass = ctx.cumulative_pass_nr();
    c.collect(ctx, pass);
    let key = picture.key();
    if let Some((k, slot, used)) = &mut c.whole
        && *k == key
    {
        *used = pass;
        return match slot {
            Slot::Ready { tex, .. } => Some(tex.id),
            Slot::Loading | Slot::Failed => None,
        };
    }
    let slot = match c.ask(ctx, key, Kind::Whole, picture.shared()) {
        Some(true) => Slot::Loading,
        Some(false) => {
            ctx.request_repaint();
            return None;
        }
        None => Slot::Failed,
    };
    let loading = c.faces.loading() + usize::from(matches!(slot, Slot::Loading));
    c.whole = Some((key, slot, pass));
    c.pending.store(loading, Ordering::Relaxed);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: usize = (SIDE * SIDE * 4) as usize;

    fn ready(lru: &mut Lru<()>, key: u64, pass: u64) {
        lru.put(
            key,
            Slot::Ready {
                tex: (),
                bytes: ONE,
                since: 0.0,
            },
            pass,
        );
    }

    #[test]
    fn the_cache_drops_the_least_recently_drawn_over_its_budget() {
        let mut lru = Lru::default();
        for k in 0..80 {
            ready(&mut lru, k, k);
        }
        // Drawn again just now: the oldest of them stays.
        lru.touch(0, 100);
        lru.evict(100);
        assert!(lru.bytes() <= BUDGET, "{} bytes kept", lru.bytes());
        assert_eq!(lru.len(), BUDGET / ONE);
        assert!(lru.touch(0, 100).is_some(), "drawn in this pass");
        assert!(
            lru.touch(1, 100).is_none(),
            "the least recently drawn went first"
        );
        assert!(
            lru.touch(79, 100).is_some(),
            "the most recently drawn stays"
        );
    }

    #[test]
    fn pictures_still_decoding_or_drawn_in_this_pass_are_kept() {
        let mut lru = Lru::default();
        for k in 0..60 {
            ready(&mut lru, k, 7);
        }
        lru.put(1000, Slot::Loading, 0);
        lru.evict(7);
        assert_eq!(lru.len(), 61, "everything was drawn in this pass");
        lru.evict(8);
        assert!(lru.bytes() <= BUDGET);
        assert!(
            lru.touch(1000, 8).is_some(),
            "a picture still decoding stays"
        );
    }

    #[test]
    fn failed_pictures_are_bounded_too() {
        let mut lru: Lru<()> = Lru::default();
        for k in 0..(MAX_ENTRIES as u64 + 50) {
            lru.put(k, Slot::Failed, k);
        }
        lru.evict(10_000);
        assert_eq!(lru.len(), MAX_ENTRIES);
        assert_eq!(lru.bytes(), 0);
    }

    #[test]
    fn a_new_picture_under_the_same_key_replaces_its_bytes() {
        let mut lru = Lru::default();
        ready(&mut lru, 1, 0);
        ready(&mut lru, 1, 1);
        assert_eq!(lru.bytes(), ONE);
    }

    #[test]
    fn an_avatar_is_a_centred_square_of_at_most_192() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(600, 300, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (size, rgba) = decode(&png, Kind::Avatar).unwrap();
        assert_eq!(size, [192, 192]);
        assert_eq!(rgba.len(), ONE);
        let (size, _) = decode(&png, Kind::Whole).unwrap();
        assert_eq!(size, [600, 300]);
        assert!(decode(b"not a picture", Kind::Avatar).is_err());
    }
}
