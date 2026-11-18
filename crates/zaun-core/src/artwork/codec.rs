//! `ImageCodec`: the only door to the `image` crate (ADR-0009, THREAT_MODEL T-18).
//!
//! Every input is untrusted. The format is sniffed from magic bytes (never from a file
//! extension), the byte limit is checked before anything is parsed, and the dimensions from
//! the header are checked against the pixel limit **before** the full decode allocates the
//! pixel buffer. The full decode then runs with `image::Limits` as a second fence and inside
//! `catch_unwind`, so a decoder panic becomes [`CodecError::Panicked`] (`ZN-ART-003`).

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::error::{Error, ErrorCode};

/// Input limits (LOGIC_SPECS §14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLimits {
    /// Maximum encoded size in bytes.
    pub max_bytes: u64,
    /// Maximum `width × height`.
    pub max_pixels: u64,
    /// Maximum width or height.
    pub max_side: u32,
}

impl ImageLimits {
    /// 32 MiB, 64 megapixels, 16 384 px per side.
    pub const DEFAULT: ImageLimits = ImageLimits {
        max_bytes: 32 * 1024 * 1024,
        max_pixels: 64 * 1024 * 1024,
        max_side: 16_384,
    };
}

impl Default for ImageLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Accepted input formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// WebP (lossy and lossless).
    WebP,
    /// Windows bitmap.
    Bmp,
    /// Windows icon (the largest entry is used).
    Ico,
}

impl ImageFormat {
    fn to_image(self) -> image::ImageFormat {
        match self {
            ImageFormat::Png => image::ImageFormat::Png,
            ImageFormat::Jpeg => image::ImageFormat::Jpeg,
            ImageFormat::WebP => image::ImageFormat::WebP,
            ImageFormat::Bmp => image::ImageFormat::Bmp,
            ImageFormat::Ico => image::ImageFormat::Ico,
        }
    }
}

/// Identify the format from the first bytes. Extensions are never consulted.
pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else if bytes.len() >= 14 && bytes.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else if bytes.len() >= 6 && bytes[..4] == [0, 0, 1, 0] && (bytes[4] != 0 || bytes[5] != 0) {
        Some(ImageFormat::Ico)
    } else {
        None
    }
}

/// Header facts, read without decoding pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    /// Sniffed format.
    pub format: ImageFormat,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Decoded straight (non-premultiplied) RGBA8 pixels, row-major.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width × height × 4` bytes.
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for RgbaImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RgbaImage({}×{})", self.width, self.height)
    }
}

impl RgbaImage {
    /// A single-colour image.
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let n = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(n * 4);
        for _ in 0..n {
            pixels.extend_from_slice(&rgba);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    fn into_buffer(self) -> image::RgbaImage {
        // The length invariant holds for every value this module builds.
        image::RgbaImage::from_raw(self.width, self.height, self.pixels)
            .unwrap_or_else(|| image::RgbaImage::new(0, 0))
    }

    fn from_buffer(b: image::RgbaImage) -> Self {
        Self {
            width: b.width(),
            height: b.height(),
            pixels: b.into_raw(),
        }
    }
}

/// Why an image was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// Not one of the accepted formats (`ZN-ART-001`).
    #[error("unsupported image format")]
    Unsupported,
    /// Over a byte, side or pixel limit (`ZN-ART-002`).
    #[error("image too large: {0}")]
    TooLarge(String),
    /// The data is malformed (`ZN-ART-001`, "could not be decoded").
    #[error("malformed image: {0}")]
    Malformed(String),
    /// The decoder panicked (`ZN-ART-003`).
    #[error("image decoder failed")]
    Panicked,
    /// Encoding the normalised image failed (`ZN-ART-003`).
    #[error("encoding failed: {0}")]
    Encode(String),
}

impl From<CodecError> for Error {
    fn from(e: CodecError) -> Self {
        let code = match &e {
            CodecError::Unsupported | CodecError::Malformed(_) => ErrorCode::ArtUnsupportedFormat,
            CodecError::TooLarge(_) => ErrorCode::ArtTooLarge,
            CodecError::Panicked | CodecError::Encode(_) => ErrorCode::ArtUnreadable,
        };
        Error::new(code).with_source(e)
    }
}

/// Decoding and encoding with limits. Implementations are pure (no I/O) and thread-safe.
pub trait ImageCodec: Send + Sync {
    /// Sniff and read the header; checks the byte, side and pixel limits.
    fn probe(&self, bytes: &[u8], limits: &ImageLimits) -> Result<ImageInfo, CodecError>;
    /// [`ImageCodec::probe`], then a full decode to RGBA8 (ICO: the largest entry).
    fn decode(&self, bytes: &[u8], limits: &ImageLimits) -> Result<RgbaImage, CodecError>;
    /// Encode as PNG.
    fn encode_png(&self, image: &RgbaImage) -> Result<Vec<u8>, CodecError>;
    /// Encode as baseline JPEG (alpha dropped) with `quality` 1–100.
    fn encode_jpeg(&self, image: &RgbaImage, quality: u8) -> Result<Vec<u8>, CodecError>;
}

/// The production codec over the `image` crate (memory-safe decoders only).
#[derive(Clone, Copy, Debug, Default)]
pub struct ImageCrateCodec;

fn check_dims(w: u32, h: u32, limits: &ImageLimits) -> Result<(), CodecError> {
    if w == 0 || h == 0 {
        return Err(CodecError::Malformed("zero dimension".into()));
    }
    if w > limits.max_side || h > limits.max_side {
        return Err(CodecError::TooLarge(format!("{w}×{h} px")));
    }
    if u64::from(w) * u64::from(h) > limits.max_pixels {
        return Err(CodecError::TooLarge(format!("{w}×{h} px")));
    }
    Ok(())
}

fn image_limits(limits: &ImageLimits) -> image::Limits {
    let mut l = image::Limits::default();
    l.max_image_width = Some(limits.max_side);
    l.max_image_height = Some(limits.max_side);
    // The RGBA buffer plus decoder working memory.
    l.max_alloc = Some(limits.max_pixels.saturating_mul(4).saturating_add(64 << 20));
    l
}

fn map_image_err(e: image::ImageError) -> CodecError {
    match e {
        image::ImageError::Limits(l) => CodecError::TooLarge(l.to_string()),
        image::ImageError::Unsupported(u) => CodecError::Malformed(u.to_string()),
        other => CodecError::Malformed(other.to_string()),
    }
}

fn guarded<T>(f: impl FnOnce() -> Result<T, CodecError>) -> Result<T, CodecError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(CodecError::Panicked))
}

impl ImageCodec for ImageCrateCodec {
    fn probe(&self, bytes: &[u8], limits: &ImageLimits) -> Result<ImageInfo, CodecError> {
        if bytes.len() as u64 > limits.max_bytes {
            return Err(CodecError::TooLarge(format!("{} bytes", bytes.len())));
        }
        let format = sniff(bytes).ok_or(CodecError::Unsupported)?;
        let (width, height) = guarded(|| {
            let mut r = image::ImageReader::with_format(Cursor::new(bytes), format.to_image());
            r.limits(image_limits(limits));
            r.into_dimensions().map_err(map_image_err)
        })?;
        check_dims(width, height, limits)?;
        Ok(ImageInfo {
            format,
            width,
            height,
        })
    }

    fn decode(&self, bytes: &[u8], limits: &ImageLimits) -> Result<RgbaImage, CodecError> {
        let info = self.probe(bytes, limits)?;
        guarded(|| {
            let mut r = image::ImageReader::with_format(Cursor::new(bytes), info.format.to_image());
            r.limits(image_limits(limits));
            let img = r.decode().map_err(map_image_err)?;
            check_dims(img.width(), img.height(), limits)?;
            Ok(RgbaImage::from_buffer(img.into_rgba8()))
        })
    }

    fn encode_png(&self, image: &RgbaImage) -> Result<Vec<u8>, CodecError> {
        let mut out = Vec::new();
        let enc = image::codecs::png::PngEncoder::new_with_quality(
            &mut out,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        );
        image::ImageEncoder::write_image(
            enc,
            &image.pixels,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| CodecError::Encode(e.to_string()))?;
        Ok(out)
    }

    fn encode_jpeg(&self, image: &RgbaImage, quality: u8) -> Result<Vec<u8>, CodecError> {
        let rgb: Vec<u8> = image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let mut out = Vec::new();
        let enc =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
        image::ImageEncoder::write_image(
            enc,
            &rgb,
            image.width,
            image.height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| CodecError::Encode(e.to_string()))?;
        Ok(out)
    }
}

// ---- Pixel operations (used by import and thumbnails) ------------------------------------

/// Copy the rectangle `(x, y, w, h)`; clamped to the image.
pub fn crop(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    let x = x.min(img.width);
    let y = y.min(img.height);
    let w = w.min(img.width - x);
    let h = h.min(img.height - y);
    let stride = img.width as usize * 4;
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y..y + h {
        let start = row as usize * stride + x as usize * 4;
        pixels.extend_from_slice(&img.pixels[start..start + w as usize * 4]);
    }
    RgbaImage {
        width: w,
        height: h,
        pixels,
    }
}

/// Lanczos3 resize to exactly `w × h` (no-op when the size already matches).
pub fn resize(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if img.width == w && img.height == h {
        return img.clone();
    }
    let buf = img.clone().into_buffer();
    RgbaImage::from_buffer(image::imageops::resize(
        &buf,
        w.max(1),
        h.max(1),
        image::imageops::FilterType::Lanczos3,
    ))
}

/// Place the image centred on a transparent square canvas of side `max(w, h)`.
pub fn pad_square(img: &RgbaImage) -> RgbaImage {
    let side = img.width.max(img.height);
    if img.width == img.height {
        return img.clone();
    }
    let mut out = RgbaImage::filled(side, side, [0, 0, 0, 0]);
    let ox = (side - img.width) / 2;
    let oy = (side - img.height) / 2;
    let src_stride = img.width as usize * 4;
    let dst_stride = side as usize * 4;
    for row in 0..img.height as usize {
        let s = row * src_stride;
        let d = (row + oy as usize) * dst_stride + ox as usize * 4;
        out.pixels[d..d + src_stride].copy_from_slice(&img.pixels[s..s + src_stride]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        ImageCrateCodec
            .encode_png(&RgbaImage::filled(w, h, [10, 20, 30, 255]))
            .unwrap()
    }

    #[test]
    fn sniffing_ignores_extensions_and_rejects_text() {
        assert_eq!(sniff(&png(2, 2)), Some(ImageFormat::Png));
        assert_eq!(sniff(b"<html><body>Not found</body></html>"), None);
        assert_eq!(sniff(b""), None);
        assert_eq!(sniff(&[0, 0, 1, 0, 0, 0]), None, "ICO with zero entries");
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageFormat::WebP));
    }

    #[test]
    fn round_trip_png_and_jpeg() {
        let c = ImageCrateCodec;
        let img = c.decode(&png(7, 5), &ImageLimits::DEFAULT).unwrap();
        assert_eq!((img.width, img.height), (7, 5));
        assert_eq!(&img.pixels[..4], &[10, 20, 30, 255]);
        let jpg = c.encode_jpeg(&img, 88).unwrap();
        assert_eq!(sniff(&jpg), Some(ImageFormat::Jpeg));
        let back = c.decode(&jpg, &ImageLimits::DEFAULT).unwrap();
        assert_eq!((back.width, back.height), (7, 5));
    }

    #[test]
    fn byte_limit_is_checked_first() {
        let limits = ImageLimits {
            max_bytes: 10,
            ..ImageLimits::DEFAULT
        };
        assert!(matches!(
            ImageCrateCodec.probe(&png(4, 4), &limits),
            Err(CodecError::TooLarge(_))
        ));
    }

    #[test]
    fn pixel_and_side_limits() {
        let limits = ImageLimits {
            max_pixels: 100,
            ..ImageLimits::DEFAULT
        };
        assert!(matches!(
            ImageCrateCodec.decode(&png(11, 10), &limits),
            Err(CodecError::TooLarge(_))
        ));
        assert!(ImageCrateCodec.decode(&png(10, 10), &limits).is_ok());
        let limits = ImageLimits {
            max_side: 8,
            ..ImageLimits::DEFAULT
        };
        assert!(matches!(
            ImageCrateCodec.decode(&png(9, 1), &limits),
            Err(CodecError::TooLarge(_))
        ));
    }

    #[test]
    fn geometry_helpers() {
        let mut img = RgbaImage::filled(4, 2, [1, 1, 1, 255]);
        img.pixels[4..8].copy_from_slice(&[9, 9, 9, 255]);
        let c = crop(&img, 1, 0, 2, 2);
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(&c.pixels[..4], &[9, 9, 9, 255]);
        let p = pad_square(&img);
        assert_eq!((p.width, p.height), (4, 4));
        assert_eq!(&p.pixels[..4], &[0, 0, 0, 0], "padding is transparent");
        let r = resize(&img, 2, 1);
        assert_eq!((r.width, r.height), (2, 1));
        // Out-of-range crops clamp instead of panicking.
        assert_eq!(crop(&img, 10, 10, 5, 5).width, 0);
    }

    #[test]
    fn errors_map_to_art_codes() {
        let e: Error = CodecError::TooLarge("x".into()).into();
        assert_eq!(e.code, ErrorCode::ArtTooLarge);
        let e: Error = CodecError::Panicked.into();
        assert_eq!(e.code, ErrorCode::ArtUnreadable);
        let e: Error = CodecError::Unsupported.into();
        assert_eq!(e.code, ErrorCode::ArtUnsupportedFormat);
    }
}
