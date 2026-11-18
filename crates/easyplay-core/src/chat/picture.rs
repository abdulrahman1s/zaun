//! Profile pictures (ADR-0038): a friend's picture is checked as it arrives, and the user's is
//! made from a file the way the engine's own apps make it: a square, re-encoded until its
//! `data:` URI fits the engine's limit.

use std::io::Cursor;

use base64::Engine as _;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageReader, Limits};

use crate::error::{Error, Result};
use crate::model::{Picture, PictureCrop, PictureSource};

/// The longest picture URI the engine accepts for the user's own profile, prefix included
/// (`maxProfileImageSize` in chat core 7.0; the official apps resize to it).
pub const MAX_URI: usize = 12_500;
/// The longest picture URI read from a friend's or group's profile; longer ones are dropped
/// before decoding. A profile travels in one message, so a real one is far shorter.
pub const MAX_URI_IN: usize = 32 * 1024;
/// The largest side of a picture read from a friend (a picture sent by EasyPlay or the
/// official apps is at most [`SIDES`]`[0]`).
pub const MAX_SIDE: u32 = 1024;
/// The largest file read to make the user's picture.
pub const MAX_SOURCE: u64 = 20 * 1024 * 1024;
/// The largest side of a file read to make the user's picture (decoding stops before it
/// allocates anything larger).
pub const MAX_SOURCE_SIDE: u32 = 16_384;

/// The largest side of the copy the crop step shows ([`source`]).
pub const SOURCE_PREVIEW: u32 = 1024;

/// The sides tried for the user's picture, largest first.
const SIDES: [u32; 9] = [512, 384, 320, 256, 224, 192, 160, 128, 96];
/// JPEG qualities tried at each side, best first.
const QUALITIES: [u8; 4] = [85, 75, 65, 55];
/// A transparent picture stays a PNG down to this side; smaller, it is put on white and
/// sent as a JPEG.
const PNG_MIN_SIDE: u32 = 128;
/// Decoders may allocate at most this much for one file.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

const JPEG_PREFIX: &str = "data:image/jpg;base64,";
const PNG_PREFIX: &str = "data:image/png;base64,";

/// A picture from a profile's `image` (`data:image/jpg|jpeg|png;base64,…`), or `None` when
/// it is too long, not a JPEG or PNG, or larger than [`MAX_SIDE`]: a bad picture is dropped,
/// never the profile.
pub fn from_data_uri(uri: &str) -> Option<Picture> {
    if uri.len() > MAX_URI_IN {
        tracing::debug!(len = uri.len(), "chat: a profile picture is too long");
        return None;
    }
    let (head, b64) = uri.split_once(',')?;
    if !matches!(
        head,
        "data:image/jpg;base64" | "data:image/jpeg;base64" | "data:image/png;base64"
    ) {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    let (w, h) = checked_size(&bytes)?;
    (w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE).then(|| Picture::new(bytes))
}

/// A picture's `data:` URI for a profile (PNG or JPEG, by its first bytes, in the apps'
/// spelling).
pub fn data_uri(p: &Picture) -> String {
    let prefix = if p.bytes().starts_with(PNG_MAGIC) {
        PNG_PREFIX
    } else {
        JPEG_PREFIX
    };
    format!(
        "{prefix}{}",
        base64::engine::general_purpose::STANDARD.encode(p.bytes())
    )
}

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const JPEG_MAGIC: &[u8] = b"\xff\xd8\xff";

/// The size in a JPEG's or PNG's header, without decoding the pixels.
fn checked_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let format = if bytes.starts_with(PNG_MAGIC) {
        image::ImageFormat::Png
    } else if bytes.starts_with(JPEG_MAGIC) {
        image::ImageFormat::Jpeg
    } else {
        return None;
    };
    ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()
}

/// A picture file decoded the way it is shown: EXIF orientation applied. What the crop step
/// shows, so [`PictureCrop`] is in its pixels. `EP-CHT-008` when it is not a picture,
/// `EP-CHT-009` when it is larger than [`MAX_SOURCE_SIDE`] a side.
pub fn decode_upright(bytes: &[u8]) -> Result<DynamicImage> {
    let not_a_picture = |e: &dyn std::fmt::Display| err!(ChtNotAPicture, detail = e.to_string());
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| not_a_picture(&e))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let from_image = |e: image::ImageError| match e {
        image::ImageError::Limits(_) => err!(ChtPictureTooLarge, detail = e.to_string()),
        e => not_a_picture(&e),
    };
    let mut decoder = reader.into_decoder().map_err(from_image)?;
    // A file whose orientation cannot be read is shown as stored.
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(from_image)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// The user's picture from a picture file: `crop` (clamped to the picture; `None` = the
/// centre square, as the apps do), shrunk until its `data:` URI fits [`MAX_URI`]. A JPEG,
/// or a PNG when the picture has transparent pixels. Nothing of the file's metadata is kept.
pub fn make(bytes: &[u8], crop: Option<PictureCrop>) -> Result<Picture> {
    let img = decode_upright(bytes)?;
    let square = square(&img, crop);
    let side = square.width();
    let sides = SIDES
        .iter()
        .map(|s| (*s).min(side))
        .collect::<std::collections::BTreeSet<_>>();
    let largest = sides.last().copied().unwrap_or(side);
    let base = square.thumbnail_exact(largest, largest);
    if has_transparency(&base) {
        for s in sides.iter().rev().filter(|s| **s >= PNG_MIN_SIDE.min(side)) {
            let small = base.thumbnail_exact(*s, *s);
            let png = encode_png(&small)?;
            if uri_len(PNG_PREFIX, png.len()) <= MAX_URI {
                return Ok(Picture::new(png));
            }
        }
    }
    let flat = DynamicImage::ImageRgb8(on_white(&base));
    for s in sides.iter().rev() {
        let small = flat.thumbnail_exact(*s, *s).to_rgb8();
        for q in QUALITIES {
            let jpeg = encode_jpeg(&small, q)?;
            if uri_len(JPEG_PREFIX, jpeg.len()) <= MAX_URI {
                return Ok(Picture::new(jpeg));
            }
        }
    }
    Err(err!(
        ChtPictureTooLarge,
        detail = "no size fits the profile's limit"
    ))
}

/// A picture file for the crop step: its size as shown and a copy of at most
/// [`SOURCE_PREVIEW`] pixels a side (a PNG when it has transparent pixels, else a JPEG).
pub fn source(path: &std::path::Path, bytes: &[u8]) -> Result<PictureSource> {
    let img = decode_upright(bytes)?;
    let (width, height) = img.dimensions();
    let small = if width.max(height) > SOURCE_PREVIEW {
        img.thumbnail(SOURCE_PREVIEW, SOURCE_PREVIEW)
    } else {
        img
    };
    let preview = if has_transparency(&small) {
        encode_png(&small)?
    } else {
        encode_jpeg(&small.to_rgb8(), 90)?
    };
    Ok(PictureSource {
        path: path.to_owned(),
        width,
        height,
        preview: Picture::new(preview),
    })
}

fn has_transparency(img: &DynamicImage) -> bool {
    img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 255)
}

/// The square to keep: `crop` clamped to the picture, or the centre square.
fn square(img: &DynamicImage, crop: Option<PictureCrop>) -> DynamicImage {
    let (w, h) = img.dimensions();
    let most = w.min(h).max(1);
    let (x, y, side) = match crop {
        Some(c) => {
            let side = c.side.clamp(1, most);
            (c.x.min(w - side), c.y.min(h - side), side)
        }
        None => ((w - most) / 2, (h - most) / 2, most),
    };
    img.crop_imm(x, y, side, side)
}

/// Transparent pixels put on white (a JPEG has no transparency).
fn on_white(img: &DynamicImage) -> image::RgbImage {
    let rgba = img.to_rgba8();
    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let mix = |c: u8| {
            let (c, a) = (u32::from(c), u32::from(a));
            u8::try_from((c * a + 255 * (255 - a)) / 255).unwrap_or(u8::MAX)
        };
        image::Rgb([mix(r), mix(g), mix(b)])
    })
}

fn encode_jpeg(img: &image::RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(img)
        .map_err(encode_failed)?;
    Ok(out)
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let mut out = Vec::new();
    let rgba = img.to_rgba8();
    rgba.write_with_encoder(PngEncoder::new_with_quality(
        &mut out,
        CompressionType::Best,
        FilterType::Adaptive,
    ))
    .map_err(encode_failed)?;
    Ok(out)
}

fn encode_failed(e: image::ImageError) -> Error {
    err!(IntTaskFailed, detail = "encoding a profile picture").with_source(e)
}

/// The length of a `data:` URI of `len` bytes after `prefix`.
fn uri_len(prefix: &str, len: usize) -> usize {
    prefix.len() + len.div_ceil(3) * 4
}

#[cfg(test)]
mod tests;
