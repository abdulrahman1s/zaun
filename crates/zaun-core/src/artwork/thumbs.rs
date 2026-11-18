//! Thumbnails at fixed sizes (PERFORMANCE §6, ADR-0009).
//!
//! `<cache>/thumbnails/<hash16>-<w>x<h>.png`, generated lazily from the stored original with
//! a Lanczos3 downscale. Each file carries the dominant colour in a private ancillary PNG
//! chunk `epCl` right after `IHDR`, so a placeholder can be painted from the first bytes.
//!
//! [`ThumbService`] runs loads and generation on the job pool (`Interactive` for visible
//! items, `Normal` for prefetch), de-duplicates requests, honours cancellation before and
//! after the work, and hands decoded pixels back through a channel; the UI's texture cache
//! drains it once per frame. Nothing decoded is kept after hand-off.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::{Mutex, RwLock};

use super::codec::{ImageCodec, ImageLimits, RgbaImage, resize};
use super::import::dominant_colour;
use super::store::{hash16, original_path};
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{Fs, FsErrorKind};
use crate::jobs::{CancelToken, JobLabel, JobPool, JobPriority};
use crate::model::{ArtSlot, ArtworkRef, Sha256};
use crate::paths::AppPaths;

/// Job label of thumbnail work.
pub const THUMB_JOB: JobLabel = JobLabel("thumbnail");
/// Largest thumbnail file read back.
const THUMB_READ_LIMIT: u64 = 8 * 1024 * 1024;

/// The fixed thumbnail sizes (PERFORMANCE §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ThumbSize {
    /// 240 × 360 cover.
    Cover240,
    /// 480 × 720 cover.
    Cover480,
    /// 32 × 32 icon.
    Icon32,
    /// 64 × 64 icon.
    Icon64,
    /// 128 × 128 icon.
    Icon128,
    /// 1280 × 720 banner (the game page's hero image).
    Hero1280,
}

impl ThumbSize {
    /// Every size.
    pub const ALL: [ThumbSize; 6] = [
        ThumbSize::Cover240,
        ThumbSize::Cover480,
        ThumbSize::Icon32,
        ThumbSize::Icon64,
        ThumbSize::Icon128,
        ThumbSize::Hero1280,
    ];

    /// Box in pixels.
    pub const fn pixels(self) -> (u32, u32) {
        match self {
            ThumbSize::Cover240 => (240, 360),
            ThumbSize::Cover480 => (480, 720),
            ThumbSize::Icon32 => (32, 32),
            ThumbSize::Icon64 => (64, 64),
            ThumbSize::Icon128 => (128, 128),
            ThumbSize::Hero1280 => (1280, 720),
        }
    }

    /// The sizes generated for a slot.
    pub fn for_slot(slot: ArtSlot) -> &'static [ThumbSize] {
        match slot {
            ArtSlot::Cover => &[ThumbSize::Cover240, ThumbSize::Cover480],
            ArtSlot::Icon => &[ThumbSize::Icon32, ThumbSize::Icon64, ThumbSize::Icon128],
            ArtSlot::Hero => &[ThumbSize::Hero1280],
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

    /// The smallest icon size at least `px` device pixels wide.
    pub fn icon_for(px: f32) -> Self {
        if px <= 32.0 {
            ThumbSize::Icon32
        } else if px <= 64.0 {
            ThumbSize::Icon64
        } else {
            ThumbSize::Icon128
        }
    }
}

/// A thumbnail: content hash of the original and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThumbKey {
    /// Hash of the stored original.
    pub art_hash: Sha256,
    /// Size.
    pub size: ThumbSize,
}

/// `<thumbs_dir>/<hash16>-<w>x<h>.png`.
pub fn thumb_path(thumbs_dir: &Path, key: &ThumbKey) -> PathBuf {
    let (w, h) = key.size.pixels();
    thumbs_dir.join(format!("{}-{w}x{h}.png", hash16(&key.art_hash)))
}

/// Decoded thumbnail pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbPixels {
    /// Which thumbnail.
    pub key: ThumbKey,
    /// Pixels (fit inside the size's box, never upscaled).
    pub image: RgbaImage,
    /// Dominant colour.
    pub dominant: [u8; 3],
}

// ---- The `epCl` chunk ---------------------------------------------------------------------

const CHUNK: &[u8; 4] = b"epCl";

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Insert the dominant-colour chunk after `IHDR` of an encoded PNG.
pub fn with_colour_chunk(png: &[u8], rgb: [u8; 3]) -> Vec<u8> {
    // Signature (8) + IHDR chunk (4 len + 4 type + 13 data + 4 crc).
    const AFTER_IHDR: usize = 33;
    if png.len() < AFTER_IHDR || &png[12..16] != b"IHDR" {
        return png.to_vec();
    }
    let mut out = Vec::with_capacity(png.len() + 15);
    out.extend_from_slice(&png[..AFTER_IHDR]);
    out.extend_from_slice(&3u32.to_be_bytes());
    let mut body = Vec::with_capacity(7);
    body.extend_from_slice(CHUNK);
    body.extend_from_slice(&rgb);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
    out.extend_from_slice(&png[AFTER_IHDR..]);
    out
}

/// Read the dominant-colour chunk (scans chunks up to `IDAT`).
pub fn read_colour_chunk(png: &[u8]) -> Option<[u8; 3]> {
    let mut i = 8usize;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().ok()?) as usize;
        let ty = &png[i + 4..i + 8];
        if ty == b"IDAT" {
            return None;
        }
        let end = i.checked_add(12)?.checked_add(len)?;
        if end > png.len() {
            return None;
        }
        if ty == CHUNK && len == 3 {
            let body = &png[i + 4..i + 11];
            let crc = u32::from_be_bytes(png[i + 11..i + 15].try_into().ok()?);
            return (crc32(body) == crc).then(|| [body[4], body[5], body[6]]);
        }
        i = end;
    }
    None
}

// ---- Generation and loading ---------------------------------------------------------------

/// Downscale `original` to fit `size` (never upscale), keeping the aspect.
pub fn scale_to(original: &RgbaImage, size: ThumbSize) -> RgbaImage {
    let (bw, bh) = size.pixels();
    let (w, h) = (original.width.max(1), original.height.max(1));
    if w <= bw && h <= bh {
        return original.clone();
    }
    let s = (f64::from(bw) / f64::from(w)).min(f64::from(bh) / f64::from(h));
    let ow = (f64::from(w) * s).round().clamp(1.0, f64::from(bw)) as u32;
    let oh = (f64::from(h) * s).round().clamp(1.0, f64::from(bh)) as u32;
    resize(original, ow, oh)
}

/// Decode `original_file`, write every `sizes` thumbnail atomically and return the pixels
/// of `want` (if listed).
pub fn generate(
    fs: &dyn Fs,
    codec: &dyn ImageCodec,
    original_file: &Path,
    thumbs_dir: &Path,
    art_hash: Sha256,
    sizes: &[ThumbSize],
    want: Option<ThumbSize>,
) -> Result<Option<ThumbPixels>> {
    let bytes = fs.read(original_file, ImageLimits::DEFAULT.max_bytes)?;
    let original = codec.decode(&bytes, &ImageLimits::DEFAULT)?;
    drop(bytes);
    let dominant = dominant_colour(&original);
    fs.create_dir_all(thumbs_dir, 0o700)?;
    let mut out = None;
    for &size in sizes {
        let key = ThumbKey { art_hash, size };
        let img = scale_to(&original, size);
        let png = with_colour_chunk(&codec.encode_png(&img)?, dominant);
        fs.atomic_write(&thumb_path(thumbs_dir, &key), &png, 0o600)?;
        if want == Some(size) {
            out = Some(ThumbPixels {
                key,
                image: img,
                dominant,
            });
        }
    }
    Ok(out)
}

/// The dominant colour stored in the header of any existing thumbnail of `art_hash` (no
/// decoding; `None` when no thumbnail carries one yet).
pub fn stored_colour(fs: &dyn Fs, thumbs_dir: &Path, art_hash: &Sha256) -> Option<[u8; 3]> {
    ThumbSize::ALL.iter().find_map(|&size| {
        let p = thumb_path(
            thumbs_dir,
            &ThumbKey {
                art_hash: *art_hash,
                size,
            },
        );
        fs.read(&p, THUMB_READ_LIMIT)
            .ok()
            .and_then(|b| read_colour_chunk(&b))
    })
}

/// Read an existing thumbnail; `Ok(None)` when it is not on disk.
pub fn load(
    fs: &dyn Fs,
    codec: &dyn ImageCodec,
    thumbs_dir: &Path,
    key: ThumbKey,
) -> Result<Option<ThumbPixels>> {
    let bytes = match fs.read(&thumb_path(thumbs_dir, &key), THUMB_READ_LIMIT) {
        Ok(b) => b,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let image = codec.decode(&bytes, &ImageLimits::DEFAULT)?;
    let dominant = read_colour_chunk(&bytes).unwrap_or_else(|| dominant_colour(&image));
    Ok(Some(ThumbPixels {
        key,
        image,
        dominant,
    }))
}

// ---- Index: hash → original ---------------------------------------------------------------

/// Where the original of each content hash is, filled from `artwork` rows.
#[derive(Default)]
pub struct ArtIndex {
    map: RwLock<HashMap<Sha256, (PathBuf, ArtSlot)>>,
}

impl ArtIndex {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the whole index.
    pub fn replace_all(&self, paths: &AppPaths, rows: &[ArtworkRef]) {
        let mut map = self.map.write();
        map.clear();
        for r in rows {
            map.insert(r.hash, (original_path(paths, r), r.slot));
        }
    }

    /// Add or update one row.
    pub fn insert(&self, paths: &AppPaths, row: &ArtworkRef) {
        self.map
            .write()
            .insert(row.hash, (original_path(paths, row), row.slot));
    }

    /// Add a picture that is not in Zaun's store: a cover in Steam's own cache
    /// (ADR-0031 §3), referenced where it is.
    pub fn insert_path(&self, hash: Sha256, path: PathBuf, slot: ArtSlot) {
        self.map.write().insert(hash, (path, slot));
    }

    /// Forget a hash.
    pub fn remove(&self, hash: &Sha256) {
        self.map.write().remove(hash);
    }

    /// Original file and slot of `hash`.
    pub fn get(&self, hash: &Sha256) -> Option<(PathBuf, ArtSlot)> {
        self.map.read().get(hash).cloned()
    }
}

// ---- Service ------------------------------------------------------------------------------

/// The outcome of a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThumbResult {
    /// Pixels ready for upload.
    Ready(ThumbPixels),
    /// Nothing can be shown (unknown hash, unreadable original); keep the placeholder.
    Failed {
        /// Which thumbnail.
        key: ThumbKey,
        /// Error code.
        code: ErrorCode,
    },
}

type Wake = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    pool: Arc<JobPool>,
    fs: Arc<dyn Fs>,
    codec: Arc<dyn ImageCodec>,
    thumbs_dir: PathBuf,
    index: Arc<ArtIndex>,
    tx: Sender<ThumbResult>,
    rx: Receiver<ThumbResult>,
    /// Key → (request generation, token).
    inflight: Mutex<HashMap<ThumbKey, (u64, CancelToken)>>,
    next_gen: std::sync::atomic::AtomicU64,
    wake: Wake,
}

/// Loads and generates thumbnails on the job pool. Cheap to clone.
#[derive(Clone)]
pub struct ThumbService {
    inner: Arc<Inner>,
}

/// Dependencies of [`ThumbService`].
pub struct ThumbDeps {
    /// Job pool.
    pub pool: Arc<JobPool>,
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Codec.
    pub codec: Arc<dyn ImageCodec>,
    /// `<cache>/thumbnails`.
    pub thumbs_dir: PathBuf,
    /// Hash → original.
    pub index: Arc<ArtIndex>,
    /// Called after each result is queued (the UI passes `ctx.request_repaint()`).
    pub wake: Wake,
}

impl ThumbService {
    /// A service.
    pub fn new(deps: ThumbDeps) -> Self {
        let (tx, rx) = unbounded();
        Self {
            inner: Arc::new(Inner {
                pool: deps.pool,
                fs: deps.fs,
                codec: deps.codec,
                thumbs_dir: deps.thumbs_dir,
                index: deps.index,
                tx,
                rx,
                inflight: Mutex::new(HashMap::new()),
                next_gen: std::sync::atomic::AtomicU64::new(0),
                wake: deps.wake,
            }),
        }
    }

    /// The index the service resolves hashes with.
    pub fn index(&self) -> &Arc<ArtIndex> {
        &self.inner.index
    }

    /// Ask for `key`; a request already in flight is not repeated. Never blocks.
    pub fn request(&self, key: ThumbKey, priority: JobPriority) {
        let (generation, token) = {
            let mut inflight = self.inner.inflight.lock();
            if inflight.contains_key(&key) {
                return;
            }
            let t = CancelToken::new();
            let g = self
                .inner
                .next_gen
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            inflight.insert(key, (g, t.clone()));
            (g, t)
        };
        let inner = Arc::clone(&self.inner);
        let _ = self
            .inner
            .pool
            .submit(priority, THUMB_JOB, move |pool_token| {
                let cancelled = || token.is_cancelled() || pool_token.is_cancelled();
                let result = if cancelled() {
                    None
                } else {
                    Some(run(&inner, key))
                };
                let still_wanted = {
                    let mut inflight = inner.inflight.lock();
                    // Only this request's own entry is removed (a newer request may have
                    // replaced a cancelled one).
                    let mine = inflight.get(&key).is_some_and(|(g, _)| *g == generation);
                    if mine {
                        inflight.remove(&key);
                    }
                    mine && !cancelled()
                };
                if let Some(r) = result.filter(|_| still_wanted) {
                    let _ = inner.tx.send(r);
                    (inner.wake)();
                }
            });
    }

    /// Drop interest in `key`: a queued job does no work; a running one discards its result.
    pub fn cancel(&self, key: ThumbKey) {
        if let Some((_, t)) = self.inner.inflight.lock().remove(&key) {
            t.cancel();
        }
    }

    /// A finished request, if any.
    pub fn try_recv(&self) -> Option<ThumbResult> {
        self.inner.rx.try_recv().ok()
    }

    /// Requests not yet answered.
    pub fn in_flight(&self) -> usize {
        self.inner.inflight.lock().len()
    }

    /// Generate every thumbnail of a freshly imported original (LOGIC_SPECS §14 last step),
    /// at `Background` priority.
    pub fn pregenerate(&self, row: &ArtworkRef, original: PathBuf) {
        let inner = Arc::clone(&self.inner);
        let (hash, slot) = (row.hash, row.slot);
        let _ = self
            .inner
            .pool
            .submit(JobPriority::Background, THUMB_JOB, move |t| {
                if t.is_cancelled() {
                    return;
                }
                let sizes = ThumbSize::for_slot(slot);
                if let Err(e) = generate(
                    &*inner.fs,
                    &*inner.codec,
                    &original,
                    &inner.thumbs_dir,
                    hash,
                    sizes,
                    None,
                ) {
                    tracing::warn!(code = %e.code, "thumbnail generation failed");
                }
            });
    }
}

fn run(inner: &Inner, key: ThumbKey) -> ThumbResult {
    let res = (|| -> Result<ThumbPixels> {
        if let Some(p) = load(&*inner.fs, &*inner.codec, &inner.thumbs_dir, key)? {
            return Ok(p);
        }
        let (original, slot) = inner
            .index
            .get(&key.art_hash)
            .ok_or_else(|| Error::new(ErrorCode::ArtUnreadable).with("reason", "unknown hash"))?;
        // Generate the sizes of the slot (plus the requested one, should it differ).
        let mut sizes: Vec<ThumbSize> = ThumbSize::for_slot(slot).to_vec();
        if !sizes.contains(&key.size) {
            sizes.push(key.size);
        }
        generate(
            &*inner.fs,
            &*inner.codec,
            &original,
            &inner.thumbs_dir,
            key.art_hash,
            &sizes,
            Some(key.size),
        )?
        .ok_or_else(|| Error::new(ErrorCode::ArtUnreadable))
    })();
    match res {
        Ok(p) => ThumbResult::Ready(p),
        Err(e) => {
            tracing::debug!(code = %e.code, "thumbnail unavailable");
            ThumbResult::Failed { key, code: e.code }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwork::ImageCrateCodec;

    #[test]
    fn colour_chunk_round_trip_and_still_decodes() {
        let c = ImageCrateCodec;
        let png = c
            .encode_png(&RgbaImage::filled(3, 2, [1, 2, 3, 255]))
            .unwrap();
        assert_eq!(read_colour_chunk(&png), None);
        let tagged = with_colour_chunk(&png, [9, 8, 7]);
        assert_eq!(read_colour_chunk(&tagged), Some([9, 8, 7]));
        let img = c.decode(&tagged, &ImageLimits::DEFAULT).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        // A damaged CRC is ignored.
        let mut bad = tagged.clone();
        bad[33 + 8] ^= 0xFF;
        assert_eq!(read_colour_chunk(&bad), None);
    }

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn scale_never_upscales_and_fits_the_box() {
        let small = RgbaImage::filled(100, 150, [0; 4]);
        assert_eq!(scale_to(&small, ThumbSize::Cover240).width, 100);
        let big = RgbaImage::filled(1200, 1800, [0; 4]);
        let t = scale_to(&big, ThumbSize::Cover240);
        assert_eq!((t.width, t.height), (240, 360));
        let wide = RgbaImage::filled(512, 256, [0; 4]);
        let t = scale_to(&wide, ThumbSize::Icon64);
        assert_eq!((t.width, t.height), (64, 32));
    }

    #[test]
    fn size_selection() {
        assert_eq!(ThumbSize::cover_for(200.0), ThumbSize::Cover240);
        assert_eq!(ThumbSize::cover_for(241.0), ThumbSize::Cover480);
        assert_eq!(ThumbSize::icon_for(48.0), ThumbSize::Icon64);
        assert_eq!(ThumbSize::icon_for(200.0), ThumbSize::Icon128);
    }
}
