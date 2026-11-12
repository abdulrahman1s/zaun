//! Import: validate, decode, normalise, store, commit (LOGIC_SPECS §14, ADR-0009).
//!
//! [`normalise`] is pure CPU work for the job pool. [`store_and_commit`] writes the file
//! atomically under its content-addressed name, upserts the row and removes the previous
//! file of the slot. [`import`] runs both. On any failure before the upsert the previous
//! artwork stays.

use std::path::Path;

use sha2::{Digest, Sha256 as Sha};

use super::codec::{ImageCodec, ImageLimits, RgbaImage, crop, pad_square, resize};
use super::store::{original_file_name, storage_err};
use crate::clock::Timestamp;
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{Fs, FsErrorKind};
use crate::ids::GameId;
use crate::model::{ArtOrigin, ArtSlot, ArtworkRef, Sha256};
use crate::paths::AppPaths;
use crate::storage::ArtworkRepo;

/// Largest stored cover.
pub const COVER_MAX: (u32, u32) = (1200, 1800);
/// Largest stored icon side.
pub const ICON_MAX: u32 = 512;
/// Largest stored hero.
pub const HERO_MAX: (u32, u32) = (3200, 1000);
/// JPEG quality of heroes.
pub const HERO_JPEG_QUALITY: u8 = 88;

/// Output encoding of a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutFormat {
    /// PNG (covers, icons).
    Png,
    /// JPEG (heroes).
    Jpeg,
}

impl OutFormat {
    /// File extension.
    pub const fn ext(self) -> &'static str {
        match self {
            OutFormat::Png => "png",
            OutFormat::Jpeg => "jpg",
        }
    }
}

/// How an input of a given size becomes the stored picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// Centre crop `(x, y, w, h)` applied first.
    pub crop: (u32, u32, u32, u32),
    /// Pad the crop to a transparent square (icons).
    pub pad_square: bool,
    /// Final size.
    pub out: (u32, u32),
    /// Encoding.
    pub format: OutFormat,
}

/// Centre crop of `w × h` to the aspect `aw : ah`.
fn centre_crop(w: u32, h: u32, aw: u32, ah: u32) -> (u32, u32, u32, u32) {
    let (w64, h64, aw64, ah64) = (u64::from(w), u64::from(h), u64::from(aw), u64::from(ah));
    if w64 * ah64 > h64 * aw64 {
        // Too wide: keep the height.
        let cw = ((h64 * aw64 + ah64 / 2) / ah64).clamp(1, w64) as u32;
        ((w - cw) / 2, 0, cw, h)
    } else {
        let ch = ((w64 * ah64 + aw64 / 2) / aw64).clamp(1, h64) as u32;
        (0, (h - ch) / 2, w, ch)
    }
}

/// Scale `(w, h)` down (never up) to fit in `(mw, mh)`, keeping the aspect.
fn fit_down(w: u32, h: u32, mw: u32, mh: u32) -> (u32, u32) {
    if w <= mw && h <= mh {
        return (w, h);
    }
    let s = (f64::from(mw) / f64::from(w)).min(f64::from(mh) / f64::from(h));
    let ow = (f64::from(w) * s).round().clamp(1.0, f64::from(mw)) as u32;
    let oh = (f64::from(h) * s).round().clamp(1.0, f64::from(mh)) as u32;
    (ow, oh)
}

/// The normalisation geometry of LOGIC_SPECS §14 for an input of `w × h` (both ≥ 1).
pub fn geometry(slot: ArtSlot, w: u32, h: u32) -> Geometry {
    let (w, h) = (w.max(1), h.max(1));
    match slot {
        ArtSlot::Cover => {
            let c = centre_crop(w, h, 2, 3);
            // An exact 2:3 crop larger than the cap becomes exactly the cap.
            let out = if c.2 > COVER_MAX.0 || c.3 > COVER_MAX.1 {
                if c.2 * 3 == c.3 * 2 {
                    COVER_MAX
                } else {
                    fit_down(c.2, c.3, COVER_MAX.0, COVER_MAX.1)
                }
            } else {
                (c.2, c.3)
            };
            Geometry {
                crop: c,
                pad_square: false,
                out,
                format: OutFormat::Png,
            }
        }
        ArtSlot::Icon => {
            let side = w.max(h).min(ICON_MAX);
            Geometry {
                crop: (0, 0, w, h),
                pad_square: true,
                out: (side, side),
                format: OutFormat::Png,
            }
        }
        ArtSlot::Hero => {
            let c = centre_crop(w, h, 32, 10);
            let out = if c.2 > HERO_MAX.0 || c.3 > HERO_MAX.1 {
                if c.2 * 10 == c.3 * 32 {
                    HERO_MAX
                } else {
                    fit_down(c.2, c.3, HERO_MAX.0, HERO_MAX.1)
                }
            } else {
                (c.2, c.3)
            };
            Geometry {
                crop: c,
                pad_square: false,
                out,
                format: OutFormat::Jpeg,
            }
        }
    }
}

/// Apply a geometry to decoded pixels.
pub fn apply_geometry(img: &RgbaImage, g: &Geometry) -> RgbaImage {
    let (x, y, w, h) = g.crop;
    let mut cur = if (x, y, w, h) == (0, 0, img.width, img.height) {
        img.clone()
    } else {
        crop(img, x, y, w, h)
    };
    if g.pad_square {
        cur = pad_square(&cur);
    }
    resize(&cur, g.out.0, g.out.1)
}

/// The most common colour (alpha-weighted, 4-bit-per-channel buckets over at most 64 × 64
/// samples), averaged within its bucket. Fully transparent images give a neutral grey.
pub fn dominant_colour(img: &RgbaImage) -> [u8; 3] {
    const NEUTRAL: [u8; 3] = [0x30, 0x30, 0x38];
    if img.width == 0 || img.height == 0 {
        return NEUTRAL;
    }
    let mut weight = vec![0u64; 4096];
    let mut sums = vec![[0u64; 3]; 4096];
    let sx = img.width.div_ceil(64).max(1);
    let sy = img.height.div_ceil(64).max(1);
    let stride = img.width as usize * 4;
    for y in (0..img.height).step_by(sy as usize) {
        for x in (0..img.width).step_by(sx as usize) {
            let i = y as usize * stride + x as usize * 4;
            let p = &img.pixels[i..i + 4];
            if p[3] < 128 {
                continue;
            }
            let b = (usize::from(p[0] >> 4) << 8)
                | (usize::from(p[1] >> 4) << 4)
                | usize::from(p[2] >> 4);
            let a = u64::from(p[3]);
            weight[b] += a;
            for c in 0..3 {
                sums[b][c] += u64::from(p[c]) * a;
            }
        }
    }
    let Some((best, &w)) = weight.iter().enumerate().max_by_key(|(_, w)| **w) else {
        return NEUTRAL;
    };
    if w == 0 {
        return NEUTRAL;
    }
    let s = sums[best];
    [(s[0] / w) as u8, (s[1] / w) as u8, (s[2] / w) as u8]
}

/// A normalised picture ready to store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Normalised {
    /// Slot.
    pub slot: ArtSlot,
    /// Encoded bytes.
    pub bytes: Vec<u8>,
    /// Encoding.
    pub format: OutFormat,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// SHA-256 of `bytes`.
    pub hash: Sha256,
    /// Dominant colour (placeholders; carried in thumbnail headers).
    pub dominant: [u8; 3],
}

/// Decode `input` with `limits` and normalise it for `slot`. Pure; run it on the job pool.
pub fn normalise(
    codec: &dyn ImageCodec,
    slot: ArtSlot,
    input: &[u8],
    limits: &ImageLimits,
) -> std::result::Result<Normalised, super::CodecError> {
    let img = codec.decode(input, limits)?;
    normalise_pixels(codec, slot, &img)
}

/// Normalise already decoded pixels (generated placeholders, extracted icons).
pub fn normalise_pixels(
    codec: &dyn ImageCodec,
    slot: ArtSlot,
    img: &RgbaImage,
) -> std::result::Result<Normalised, super::CodecError> {
    let g = geometry(slot, img.width, img.height);
    let out = apply_geometry(img, &g);
    let dominant = dominant_colour(&out);
    let bytes = match g.format {
        OutFormat::Png => codec.encode_png(&out)?,
        OutFormat::Jpeg => codec.encode_jpeg(&out, HERO_JPEG_QUALITY)?,
    };
    let hash: Sha256 = Sha::digest(&bytes).into();
    Ok(Normalised {
        slot,
        bytes,
        format: g.format,
        width: out.width,
        height: out.height,
        hash,
        dominant,
    })
}

/// Read a user-chosen file: symlinks are followed (the user picked it), at most
/// `limits.max_bytes` are read.
pub fn read_user_file(fs: &dyn Fs, path: &Path, limits: &ImageLimits) -> Result<Vec<u8>> {
    fs.read_follow(path, limits.max_bytes).map_err(|e| {
        if matches!(e.kind, FsErrorKind::TooLarge { .. }) {
            Error::new(ErrorCode::ArtTooLarge).with("path", path.display())
        } else {
            Error::from(e)
        }
    })
}

/// The result of an import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    /// The new row.
    pub art: ArtworkRef,
    /// Dominant colour.
    pub dominant: [u8; 3],
    /// The row it replaced, if any (its texture and thumbnails can be dropped).
    pub replaced: Option<ArtworkRef>,
}

/// Write `n` as `artwork/<game>/<slot>-<hash16>.<ext>` atomically, upsert the row and
/// delete the previous file of the slot (when its name differs).
#[allow(clippy::too_many_arguments)]
pub fn store_and_commit<R: ArtworkRepo + ?Sized>(
    fs: &dyn Fs,
    paths: &AppPaths,
    repo: &mut R,
    game: GameId,
    n: &Normalised,
    origin: ArtOrigin,
    now: Timestamp,
) -> Result<Imported> {
    let dir = paths.artwork_game_dir(game);
    fs.create_dir_all(&dir, 0o700)?;
    let file = original_file_name(n.slot, &n.hash, n.format.ext());
    let path = dir.join(&file);
    let already = fs
        .metadata(&path)
        .is_ok_and(|m| m.is_file() && m.size == n.bytes.len() as u64);
    if !already {
        fs.atomic_write(&path, &n.bytes, 0o600)?;
    }
    let previous = repo
        .artwork_of(game)
        .map_err(storage_err)?
        .into_iter()
        .find(|a| a.slot == n.slot);
    let art = ArtworkRef {
        game_id: game,
        slot: n.slot,
        file: file.clone(),
        hash: n.hash,
        width: n.width,
        height: n.height,
        source: origin,
        updated_at: now,
    };
    repo.upsert_artwork(&art).map_err(storage_err)?;
    if let Some(prev) = &previous
        && prev.file != file
    {
        match fs.remove_file(&dir.join(&prev.file)) {
            Ok(()) => {}
            Err(e) if e.kind == FsErrorKind::NotFound => {}
            // Left for store maintenance.
            Err(e) => tracing::warn!(error = %e, "could not remove previous artwork"),
        }
    }
    Ok(Imported {
        art,
        dominant: n.dominant,
        replaced: previous,
    })
}

/// Shared dependencies of an import.
#[derive(Clone, Copy)]
pub struct ImportDeps<'a> {
    /// Filesystem.
    pub fs: &'a dyn Fs,
    /// Codec.
    pub codec: &'a dyn ImageCodec,
    /// Paths.
    pub paths: &'a AppPaths,
    /// Input limits.
    pub limits: ImageLimits,
}

/// What to import.
#[derive(Clone, Debug)]
pub enum ImportInput<'a> {
    /// A user-chosen file (`source = file`).
    File(&'a Path),
    /// Encoded bytes from another source (executable icon, Steam CDN, generated).
    Bytes {
        /// The bytes (untrusted).
        bytes: &'a [u8],
        /// Recorded `source`.
        origin: ArtOrigin,
    },
}

/// Validate, normalise, store and commit one slot (LOGIC_SPECS §14).
pub fn import<R: ArtworkRepo + ?Sized>(
    deps: &ImportDeps<'_>,
    repo: &mut R,
    game: GameId,
    slot: ArtSlot,
    input: ImportInput<'_>,
    now: Timestamp,
) -> Result<Imported> {
    let (owned, origin);
    let bytes: &[u8] = match input {
        ImportInput::File(p) => {
            owned = read_user_file(deps.fs, p, &deps.limits)?;
            origin = ArtOrigin::File;
            &owned
        }
        ImportInput::Bytes { bytes, origin: o } => {
            origin = o;
            bytes
        }
    };
    let n = normalise(deps.codec, slot, bytes, &deps.limits)?;
    store_and_commit(deps.fs, deps.paths, repo, game, &n, origin, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (ART-002): output size and format for a matrix of input aspect ratios.
    #[test]
    fn geometry_table() {
        use ArtSlot::*;
        use OutFormat::*;
        type Row = (
            ArtSlot,
            (u32, u32),
            (u32, u32, u32, u32),
            (u32, u32),
            OutFormat,
        );
        #[rustfmt::skip]
        let table: &[Row] = &[
            // slot   input          crop                  out           format
            (Cover, (600, 900),   (0, 0, 600, 900),     (600, 900),   Png),
            (Cover, (1200, 1800), (0, 0, 1200, 1800),   (1200, 1800), Png),
            (Cover, (2400, 3600), (0, 0, 2400, 3600),   (1200, 1800), Png),
            (Cover, (1920, 1080), (600, 0, 720, 1080),  (720, 1080),  Png),
            (Cover, (4000, 1000), (1666, 0, 667, 1000), (667, 1000),  Png),
            (Cover, (1000, 4000), (0, 1250, 1000, 1500),(1000, 1500), Png),
            (Cover, (3000, 3000), (500, 0, 2000, 3000), (1200, 1800), Png),
            (Cover, (1, 1),       (0, 0, 1, 1),         (1, 1),       Png),
            (Cover, (10, 1000),   (0, 492, 10, 15),     (10, 15),     Png),
            (Icon,  (256, 256),   (0, 0, 256, 256),     (256, 256),   Png),
            (Icon,  (1024, 1024), (0, 0, 1024, 1024),   (512, 512),   Png),
            (Icon,  (48, 32),     (0, 0, 48, 32),       (48, 48),     Png),
            (Icon,  (2000, 100),  (0, 0, 2000, 100),    (512, 512),   Png),
            (Hero,  (3840, 1240), (0, 20, 3840, 1200),  (3200, 1000), Jpeg),
            (Hero,  (1920, 620),  (0, 10, 1920, 600),   (1920, 600),  Jpeg),
            (Hero,  (460, 215),   (0, 35, 460, 144),    (460, 144),   Jpeg),
            (Hero,  (1000, 1000), (0, 343, 1000, 313),  (1000, 313),  Jpeg),
            (Hero,  (9000, 1000), (2900, 0, 3200, 1000),(3200, 1000), Jpeg),
        ];
        for &(slot, (w, h), crop, out, format) in table {
            let g = geometry(slot, w, h);
            assert_eq!(g.crop, crop, "{slot:?} {w}×{h} crop");
            assert_eq!(g.out, out, "{slot:?} {w}×{h} out");
            assert_eq!(g.format, format, "{slot:?} {w}×{h} format");
            assert!(g.out.0 >= 1 && g.out.1 >= 1);
        }
    }

    #[test]
    fn cover_aspect_stays_two_by_three_for_many_sizes() {
        for w in [3u32, 17, 299, 641, 1201, 2999] {
            for h in [2u32, 31, 450, 901, 1801, 5000] {
                let g = geometry(ArtSlot::Cover, w, h);
                assert!(g.out.0 <= COVER_MAX.0 && g.out.1 <= COVER_MAX.1);
                let ratio = f64::from(g.out.0) / f64::from(g.out.1);
                if g.out.1 >= 30 {
                    assert!((ratio - 2.0 / 3.0).abs() < 0.04, "{w}×{h} → {:?}", g.out);
                }
                let g = geometry(ArtSlot::Hero, w, h);
                assert!(g.out.0 <= HERO_MAX.0 && g.out.1 <= HERO_MAX.1);
            }
        }
    }

    #[test]
    fn dominant_colour_picks_the_majority() {
        let mut img = RgbaImage::filled(10, 10, [200, 30, 30, 255]);
        for p in img.pixels.as_chunks_mut::<4>().0.iter_mut().take(30) {
            p.copy_from_slice(&[0, 0, 255, 255]);
        }
        assert_eq!(dominant_colour(&img), [200, 30, 30]);
        assert_eq!(
            dominant_colour(&RgbaImage::filled(4, 4, [255, 255, 255, 0])),
            [0x30, 0x30, 0x38]
        );
    }
}
