//! `TextureCache`: LRU by bytes, request/poll API, upload budget per frame. See MODULES §8,
//! PERFORMANCE §6, ADR-0009 and task UI-007.
//!
//! Flow per frame:
//! 1. [`TextureCache::begin_frame`].
//! 2. Screens call [`TextureCache::get`] for every visible (and prefetch) key: a ready
//!    texture is returned and pinned for the frame; otherwise the key is requested from the
//!    [`ArtSource`] once and a placeholder colour is returned.
//! 3. [`TextureCache::end_frame`] uploads decoded images within the budget (6 textures or
//!    4 MB), cancels requests nobody asked for this frame, and evicts unpinned entries in
//!    LRU order until the cache is under its byte cap.
//!
//! Decoding never happens here: pixels arrive through the source: [`disk::ThumbSource`] in
//! production (thumbnails decoded on the job pool, ART-004), [`synthetic`] for the preview
//! library and tests. Uploading goes through an [`Uploader`] so tests can count uploads
//! without a GPU.

use std::collections::{HashMap, VecDeque};

use easyplay_core::model::ArtKey;
use egui::{Color32, ColorImage, TextureHandle, TextureId, TextureOptions};

/// Thumbnail sizes (PERFORMANCE §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ThumbSize {
    /// 240 × 360 cover.
    Cover240,
    /// 480 × 720 cover.
    Cover480,
    /// 64 × 64 icon.
    Icon64,
}

impl ThumbSize {
    /// Pixel size.
    pub const fn pixels(self) -> [usize; 2] {
        match self {
            ThumbSize::Cover240 => [240, 360],
            ThumbSize::Cover480 => [480, 720],
            ThumbSize::Icon64 => [64, 64],
        }
    }

    /// The smallest cover size at least `px_w` device pixels wide.
    pub fn cover_for(px_w: f32) -> Self {
        if px_w <= 240.0 {
            ThumbSize::Cover240
        } else {
            ThumbSize::Cover480
        }
    }
}

/// Cache key: content hash and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexKey {
    /// Artwork content hash.
    pub art: ArtKey,
    /// Thumbnail size.
    pub size: ThumbSize,
}

/// Decoded pixels handed to the UI thread.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// Which request.
    pub key: TexKey,
    /// RGBA pixels.
    pub image: ColorImage,
    /// Dominant colour (placeholder hand-off).
    pub dominant: Color32,
}

/// Where decoded pixels come from. Implementations never block.
pub trait ArtSource {
    /// Ask for `key` to be decoded.
    fn request(&self, key: TexKey);
    /// Drop interest in `key` (best effort; a result may still arrive).
    fn cancel(&self, key: TexKey);
    /// A finished decode, if any.
    fn try_recv(&self) -> Option<Decoded>;
}

/// Uploads pixels to the GPU.
pub trait Uploader {
    /// The handle kept alive while the texture is cached.
    type Handle;
    /// Upload.
    fn upload(&mut self, key: TexKey, image: ColorImage) -> Self::Handle;
    /// The id to paint with.
    fn id(handle: &Self::Handle) -> TextureId;
}

/// The real uploader: `Context::load_texture` [F-312]. Dropping the handle frees the texture.
pub struct EguiUploader(pub egui::Context);

impl Uploader for EguiUploader {
    type Handle = TextureHandle;
    fn upload(&mut self, key: TexKey, image: ColorImage) -> TextureHandle {
        let name = format!(
            "art-{:02x}{:02x}-{:?}",
            key.art.0[0], key.art.0[1], key.size
        );
        self.0.load_texture(name, image, TextureOptions::LINEAR)
    }
    fn id(handle: &TextureHandle) -> TextureId {
        handle.id()
    }
}

/// Budgets and capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum uploads per frame.
    pub uploads_per_frame: usize,
    /// Maximum uploaded bytes per frame.
    pub bytes_per_frame: usize,
    /// Cache capacity in bytes.
    pub capacity: usize,
    /// Decoded images waiting for upload, at most (older ones are dropped and re-requested).
    pub pending_max: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            uploads_per_frame: 6,
            bytes_per_frame: 4 * 1024 * 1024,
            capacity: 128 * 1024 * 1024,
            pending_max: 64,
        }
    }
}

/// What a screen gets for a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexState {
    /// Paint this texture.
    Ready(TextureId),
    /// Not yet: paint the dominant colour if known, else the generated placeholder.
    Pending(Option<Color32>),
}

struct Entry<H> {
    handle: H,
    bytes: usize,
    last_used: u64,
}

/// Per-frame statistics (tests and the debug overlay).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Textures uploaded this frame.
    pub uploads: usize,
    /// Bytes uploaded this frame.
    pub bytes: usize,
    /// Entries evicted this frame.
    pub evicted: usize,
    /// Requests cancelled this frame.
    pub cancelled: usize,
}

/// The cache.
pub struct TextureCache<U: Uploader> {
    uploader: U,
    limits: Limits,
    entries: HashMap<TexKey, Entry<U::Handle>>,
    bytes: usize,
    frame: u64,
    pinned: Vec<TexKey>,
    /// Requested, not yet received: key → last frame it was asked for.
    requested: HashMap<TexKey, u64>,
    pending: VecDeque<Decoded>,
    dominant: HashMap<ArtKey, Color32>,
    stats: FrameStats,
}

impl<U: Uploader> TextureCache<U> {
    /// A cache with `limits`.
    pub fn new(uploader: U, limits: Limits) -> Self {
        Self {
            uploader,
            limits,
            entries: HashMap::new(),
            bytes: 0,
            frame: 0,
            pinned: Vec::new(),
            requested: HashMap::new(),
            pending: VecDeque::new(),
            dominant: HashMap::new(),
            stats: FrameStats::default(),
        }
    }

    /// Start a frame: clears pins and counters.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.pinned.clear();
        self.stats = FrameStats::default();
    }

    /// Record a known dominant colour (from the artwork row) for placeholders.
    pub fn set_dominant(&mut self, art: ArtKey, color: Color32) {
        self.dominant.insert(art, color);
    }

    /// Look up `key` for painting this frame; requests it when absent. `pin` keeps it from
    /// eviction for this frame (visible and prefetch rows).
    pub fn get(&mut self, key: TexKey, pin: bool, source: &dyn ArtSource) -> TexState {
        if pin {
            self.pinned.push(key);
        }
        if let Some(e) = self.entries.get_mut(&key) {
            e.last_used = self.frame;
            return TexState::Ready(U::id(&e.handle));
        }
        let in_pending = self.pending.iter().any(|d| d.key == key);
        match self.requested.get_mut(&key) {
            Some(f) => *f = self.frame,
            None if !in_pending => {
                source.request(key);
                self.requested.insert(key, self.frame);
            }
            None => {}
        }
        TexState::Pending(self.dominant.get(&key.art).copied())
    }

    /// Receive, upload within budget, cancel stale requests, evict to capacity.
    pub fn end_frame(&mut self, source: &dyn ArtSource) {
        while let Some(d) = source.try_recv() {
            if self.requested.remove(&d.key).is_none() {
                continue; // cancelled or duplicate
            }
            self.dominant.insert(d.key.art, d.dominant);
            self.pending.push_back(d);
            if self.pending.len() > self.limits.pending_max
                && let Some(old) = self.pending.pop_front()
            {
                // Dropped before upload; it will be requested again if still wanted.
                let _ = old;
            }
        }
        while let Some(d) = self.pending.front() {
            let bytes = d.image.width() * d.image.height() * 4;
            let over_count = self.stats.uploads >= self.limits.uploads_per_frame;
            let over_bytes =
                self.stats.uploads > 0 && self.stats.bytes + bytes > self.limits.bytes_per_frame;
            if over_count || over_bytes {
                break;
            }
            let Some(d) = self.pending.pop_front() else {
                break;
            };
            let handle = self.uploader.upload(d.key, d.image);
            self.stats.uploads += 1;
            self.stats.bytes += bytes;
            self.bytes += bytes;
            if let Some(old) = self.entries.insert(
                d.key,
                Entry {
                    handle,
                    bytes,
                    last_used: self.frame,
                },
            ) {
                self.bytes -= old.bytes;
            }
        }
        let frame = self.frame;
        let stale: Vec<TexKey> = self
            .requested
            .iter()
            .filter(|(_, f)| **f < frame)
            .map(|(k, _)| *k)
            .collect();
        for k in stale {
            source.cancel(k);
            self.requested.remove(&k);
            self.stats.cancelled += 1;
        }
        self.evict();
    }

    fn evict(&mut self) {
        while self.bytes > self.limits.capacity {
            let victim = self
                .entries
                .iter()
                .filter(|(k, _)| !self.pinned.contains(k))
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| *k);
            let Some(k) = victim else { break };
            if let Some(e) = self.entries.remove(&k) {
                self.bytes -= e.bytes;
                self.stats.evicted += 1;
            }
        }
    }

    /// Bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Cached textures.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Requests in flight.
    pub fn in_flight(&self) -> usize {
        self.requested.len()
    }

    /// Decoded images waiting for upload.
    pub fn waiting_upload(&self) -> usize {
        self.pending.len()
    }

    /// This frame's statistics.
    pub fn stats(&self) -> FrameStats {
        self.stats
    }
}

/// The production source: thumbnails from `<cache>/thumbnails`, generated and decoded on the
/// job pool by [`easyplay_core::artwork::ThumbService`] (ART-004).
pub mod disk {
    use easyplay_core::artwork::{ThumbKey, ThumbResult, ThumbService, ThumbSize as CoreSize};
    use easyplay_core::jobs::JobPriority;
    use egui::{Color32, ColorImage};

    use super::{ArtSource, Decoded, TexKey, ThumbSize};
    use easyplay_core::model::ArtKey;

    /// The core size for a UI size.
    pub fn core_size(size: ThumbSize) -> CoreSize {
        match size {
            ThumbSize::Cover240 => CoreSize::Cover240,
            ThumbSize::Cover480 => CoreSize::Cover480,
            ThumbSize::Icon64 => CoreSize::Icon64,
        }
    }

    fn ui_size(size: CoreSize) -> Option<ThumbSize> {
        match size {
            CoreSize::Cover240 => Some(ThumbSize::Cover240),
            CoreSize::Cover480 => Some(ThumbSize::Cover480),
            CoreSize::Icon64 => Some(ThumbSize::Icon64),
            CoreSize::Icon32 | CoreSize::Icon128 => None,
        }
    }

    /// [`ArtSource`] over a [`ThumbService`]. Build the service with
    /// `wake: Arc::new(move || ctx.request_repaint())` so results arrive promptly.
    pub struct ThumbSource {
        service: ThumbService,
        priority: JobPriority,
    }

    impl ThumbSource {
        /// Requests at `Interactive` priority (the cache asks only for visible and prefetch
        /// rows, and cancels the rest each frame).
        pub fn new(service: ThumbService) -> Self {
            Self {
                service,
                priority: JobPriority::Interactive,
            }
        }

        /// The service (to update its index after imports).
        pub fn service(&self) -> &ThumbService {
            &self.service
        }
    }

    fn key(k: TexKey) -> ThumbKey {
        ThumbKey {
            art_hash: k.art.0,
            size: core_size(k.size),
        }
    }

    impl ArtSource for ThumbSource {
        fn request(&self, k: TexKey) {
            self.service.request(key(k), self.priority);
        }
        fn cancel(&self, k: TexKey) {
            self.service.cancel(key(k));
        }
        fn try_recv(&self) -> Option<Decoded> {
            // Failures are skipped: the card keeps its placeholder.
            while let Some(r) = self.service.try_recv() {
                let ThumbResult::Ready(p) = r else { continue };
                let Some(size) = ui_size(p.key.size) else {
                    continue;
                };
                let [r, g, b] = p.dominant;
                let image = ColorImage::from_rgba_unmultiplied(
                    [p.image.width as usize, p.image.height as usize],
                    &p.image.pixels,
                );
                return Some(Decoded {
                    key: TexKey {
                        art: ArtKey(p.key.art_hash),
                        size,
                    },
                    image,
                    dominant: Color32::from_rgb(r, g, b),
                });
            }
            None
        }
    }
}

/// A synthetic artwork source: generated covers for the preview library and tests.
pub mod synthetic {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::thread;

    use super::{ArtSource, Decoded, TexKey};
    use crate::theme::palette::{hsl, mix};
    use egui::{Color32, ColorImage};

    /// Generate a cover for `key`: a soft two-hue gradient with a large disc and horizon
    /// bands, deterministic per hash.
    pub fn render(key: TexKey) -> Decoded {
        let [w, h] = key.size.pixels();
        let s = &key.art.0;
        let hue = f32::from(s[0].wrapping_mul(97) ^ s[9]) / 255.0;
        let hue2 = hue + 0.12 + f32::from(s[2] % 40) / 255.0;
        let top = hsl(hue, 0.45, 0.36);
        let bottom = hsl(hue2, 0.55, 0.14);
        let sun = hsl(hue + 0.5, 0.65, 0.66);
        let (cx, cy) = (
            w as f32 * (0.3 + f32::from(s[3] % 40) / 100.0),
            h as f32 * 0.38,
        );
        let r = w as f32 * (0.16 + f32::from(s[4] % 20) / 200.0);
        let horizon = h as f32 * (0.55 + f32::from(s[5] % 15) / 100.0);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            let fy = y as f32;
            let base = mix(top, bottom, fy / h as f32);
            for x in 0..w {
                let fx = x as f32;
                let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
                let mut c = base;
                if d < r {
                    c = mix(sun, base, (d / r).powi(6));
                } else if d < r * 1.8 {
                    c = mix(c, sun, 0.12 * (1.0 - (d - r) / (r * 0.8)));
                }
                if fy > horizon {
                    let wave = ((fx / w as f32 * 6.0 + f32::from(s[6] % 7)).sin() * 6.0) + horizon;
                    if fy > wave {
                        c = mix(c, hsl(hue2 + 0.05, 0.4, 0.08), 0.7);
                    }
                }
                pixels.push(c);
            }
        }
        let image = ColorImage::new([w, h], pixels);
        Decoded {
            key,
            image,
            dominant: mix(top, bottom, 0.5),
        }
    }

    /// Generates on request, inline: deterministic for tests and snapshots.
    #[derive(Default)]
    pub struct Inline {
        ready: RefCell<VecDeque<Decoded>>,
        /// Number of requests seen.
        pub requests: RefCell<usize>,
        /// Number of cancellations seen.
        pub cancels: RefCell<usize>,
        /// Render real pixels (false: a 1 × 1 image sized as the key says, for accounting).
        pub tiny: bool,
    }

    impl Inline {
        /// Real pixels.
        pub fn new() -> Self {
            Self::default()
        }

        /// Flat images of the right size: for byte accounting without the rendering cost.
        pub fn tiny() -> Self {
            Self {
                tiny: true,
                ..Self::default()
            }
        }
    }

    impl ArtSource for Inline {
        fn request(&self, key: TexKey) {
            *self.requests.borrow_mut() += 1;
            let d = if self.tiny {
                let [w, h] = key.size.pixels();
                Decoded {
                    key,
                    image: ColorImage::filled([w, h], Color32::GRAY),
                    dominant: Color32::GRAY,
                }
            } else {
                render(key)
            };
            self.ready.borrow_mut().push_back(d);
        }
        fn cancel(&self, key: TexKey) {
            *self.cancels.borrow_mut() += 1;
            self.ready.borrow_mut().retain(|d| d.key != key);
        }
        fn try_recv(&self) -> Option<Decoded> {
            self.ready.borrow_mut().pop_front()
        }
    }

    /// Generates on a background thread and wakes the UI (`easyplay` preview).
    pub struct Threaded {
        tx: Sender<TexKey>,
        rx: Receiver<Decoded>,
    }

    impl Threaded {
        /// Start the worker; `wake` is called after each result.
        pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
            let (tx, req_rx) = channel::<TexKey>();
            let (res_tx, rx) = channel();
            // The worker blocks on its own channel; it is not the UI thread.
            #[allow(clippy::disallowed_methods)]
            let _ = thread::Builder::new()
                .name("synthetic-art".into())
                .spawn(move || {
                    while let Ok(key) = req_rx.recv() {
                        if res_tx.send(render(key)).is_err() {
                            break;
                        }
                        wake();
                    }
                });
            Self { tx, rx }
        }
    }

    impl ArtSource for Threaded {
        fn request(&self, key: TexKey) {
            let _ = self.tx.send(key);
        }
        fn cancel(&self, _key: TexKey) {}
        fn try_recv(&self) -> Option<Decoded> {
            self.rx.try_recv().ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::synthetic::Inline;
    use super::*;

    #[derive(Default)]
    struct FakeUploader {
        uploads: usize,
    }

    impl Uploader for FakeUploader {
        type Handle = u64;
        fn upload(&mut self, _key: TexKey, _image: ColorImage) -> u64 {
            self.uploads += 1;
            self.uploads as u64
        }
        fn id(h: &u64) -> TextureId {
            TextureId::User(*h)
        }
    }

    fn key(i: u32, size: ThumbSize) -> TexKey {
        let mut h = [0u8; 32];
        h[..4].copy_from_slice(&i.to_le_bytes());
        TexKey {
            art: ArtKey(h),
            size,
        }
    }

    const COVER: usize = 240 * 360 * 4;

    #[test]
    fn request_then_ready_after_poll() {
        let src = Inline::tiny();
        let mut c = TextureCache::new(FakeUploader::default(), Limits::default());
        c.begin_frame();
        let k = key(1, ThumbSize::Cover240);
        assert_eq!(c.get(k, true, &src), TexState::Pending(None));
        // Asking again in the same frame does not re-request.
        c.get(k, true, &src);
        assert_eq!(*src.requests.borrow(), 1);
        c.end_frame(&src);
        c.begin_frame();
        assert!(matches!(c.get(k, true, &src), TexState::Ready(_)));
        assert_eq!(c.bytes(), COVER);
    }

    #[test]
    fn uploads_respect_the_per_frame_budget() {
        let src = Inline::tiny();
        let mut c = TextureCache::new(FakeUploader::default(), Limits::default());
        c.begin_frame();
        for i in 0..20 {
            c.get(key(i, ThumbSize::Cover240), true, &src);
        }
        c.end_frame(&src);
        assert_eq!(c.stats().uploads, 6);
        // 480 × 720 covers are 1.3 MB: the byte budget allows three.
        let mut c = TextureCache::new(FakeUploader::default(), Limits::default());
        c.begin_frame();
        for i in 0..10 {
            c.get(key(i, ThumbSize::Cover480), true, &src);
        }
        c.end_frame(&src);
        assert_eq!(c.stats().uploads, 3);
        assert!(c.stats().bytes <= Limits::default().bytes_per_frame);
    }

    #[test]
    fn lru_eviction_spares_pinned_entries() {
        let src = Inline::tiny();
        let limits = Limits {
            capacity: COVER * 4,
            ..Limits::default()
        };
        let mut c = TextureCache::new(FakeUploader::default(), limits);
        for i in 0..4 {
            c.begin_frame();
            c.get(key(i, ThumbSize::Cover240), true, &src);
            c.end_frame(&src);
        }
        assert_eq!(c.len(), 4);
        // Frame: pin 0 (oldest) and add a fifth: 1 (least recently used, unpinned) goes.
        c.begin_frame();
        c.get(key(0, ThumbSize::Cover240), true, &src);
        c.get(key(4, ThumbSize::Cover240), true, &src);
        c.end_frame(&src);
        assert_eq!(c.len(), 4);
        assert!(c.bytes() <= limits.capacity);
        c.begin_frame();
        assert!(matches!(
            c.get(key(0, ThumbSize::Cover240), false, &src),
            TexState::Ready(_)
        ));
        assert!(matches!(
            c.get(key(1, ThumbSize::Cover240), false, &src),
            TexState::Pending(_)
        ));
    }

    #[test]
    fn stale_requests_are_cancelled() {
        let src = Inline::tiny();
        let mut c = TextureCache::new(FakeUploader::default(), Limits::default());
        // A source that never answers within the frame: use a key we request and then
        // drop the result by cancelling before poll.
        struct Never(std::cell::RefCell<usize>);
        impl ArtSource for Never {
            fn request(&self, _key: TexKey) {}
            fn cancel(&self, _key: TexKey) {
                *self.0.borrow_mut() += 1;
            }
            fn try_recv(&self) -> Option<Decoded> {
                None
            }
        }
        let never = Never(Default::default());
        c.begin_frame();
        c.get(key(1, ThumbSize::Cover240), true, &never);
        c.end_frame(&never);
        assert_eq!(c.in_flight(), 1);
        c.begin_frame();
        c.end_frame(&never); // not asked for this frame
        assert_eq!(*never.0.borrow(), 1);
        assert_eq!(c.in_flight(), 0);
        let _ = src;
    }

    /// ART-004: the disk source feeds the cache from real thumbnails on the job pool; an
    /// unknown hash keeps the placeholder.
    #[test]
    fn disk_source_feeds_the_cache() {
        use std::sync::Arc;

        use easyplay_core::artwork::thumbs::{self, ArtIndex, ThumbDeps};
        use easyplay_core::artwork::{ImageCodec, ImageCrateCodec, RgbaImage, ThumbService};
        use easyplay_core::clock::SystemClock;
        use easyplay_core::jobs::{JobPool, JobPoolConfig};
        use easyplay_testkit::MemFs;

        let fs = Arc::new(MemFs::new());
        let original = std::path::Path::new("/data/artwork/g/cover-0101010101010101.png");
        let png = ImageCrateCodec
            .encode_png(&RgbaImage::filled(600, 900, [200, 10, 10, 255]))
            .unwrap();
        fs.file(original, png);
        let hash = [1u8; 32];
        let thumbs_dir = std::path::PathBuf::from("/cache/thumbnails");
        thumbs::generate(
            &*fs,
            &ImageCrateCodec,
            original,
            &thumbs_dir,
            hash,
            &[easyplay_core::artwork::ThumbSize::Cover240],
            None,
        )
        .unwrap();
        let pool = Arc::new(
            JobPool::new(
                JobPoolConfig {
                    workers: 2,
                    ..JobPoolConfig::default()
                },
                Arc::new(SystemClock::new()),
            )
            .unwrap(),
        );
        let src = disk::ThumbSource::new(ThumbService::new(ThumbDeps {
            pool,
            fs,
            codec: Arc::new(ImageCrateCodec),
            thumbs_dir,
            index: Arc::new(ArtIndex::new()),
            wake: Arc::new(|| {}),
        }));
        let mut c = TextureCache::new(FakeUploader::default(), Limits::default());
        let k = TexKey {
            art: ArtKey(hash),
            size: ThumbSize::Cover240,
        };
        let missing = key(9, ThumbSize::Cover240);
        let mut ready = false;
        for _ in 0..400 {
            c.begin_frame();
            ready = matches!(c.get(k, true, &src), TexState::Ready(_));
            c.get(missing, true, &src);
            c.end_frame(&src);
            if ready {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(ready);
        assert_eq!(c.bytes(), 240 * 360 * 4);
        c.begin_frame();
        assert_eq!(
            c.get(missing, true, &src),
            TexState::Pending(None),
            "unknown art keeps the placeholder"
        );
        assert_eq!(c.get(k, false, &src), TexState::Ready(TextureId::User(1)));
    }

    /// Acceptance (UI-007): scrolling a synthetic 5 000-item grid never uploads more than the
    /// per-frame budget and stays under the cap.
    #[test]
    fn scrolling_5000_items_stays_within_budget_and_cap() {
        let src = Inline::tiny();
        let limits = Limits {
            capacity: 32 * 1024 * 1024,
            ..Limits::default()
        };
        let mut c = TextureCache::new(FakeUploader::default(), limits);
        let (cols, rows_visible) = (6u32, 4u32);
        let total_rows = 5000 / cols;
        let mut max_uploads = 0;
        let mut max_bytes = 0;
        for frame in 0..(total_rows * 2) {
            let first_row = (frame / 2).min(total_rows - rows_visible);
            c.begin_frame();
            for row in first_row.saturating_sub(1)..(first_row + rows_visible + 1) {
                for col in 0..cols {
                    c.get(key(row * cols + col, ThumbSize::Cover240), true, &src);
                }
            }
            c.end_frame(&src);
            max_uploads = max_uploads.max(c.stats().uploads);
            max_bytes = max_bytes.max(c.stats().bytes);
            assert!(
                c.bytes() <= limits.capacity,
                "frame {frame}: {} bytes",
                c.bytes()
            );
        }
        assert!(max_uploads <= limits.uploads_per_frame);
        assert!(max_bytes <= limits.bytes_per_frame);
    }
}
