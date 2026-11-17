//! A picture's preview for the message: a small JPEG whose `data:` URI fits the engine's limit.

/// The longest side of a preview picture tried first, in pixels; smaller ones follow until
/// one fits [`PREVIEW_BUDGET`].
pub const PREVIEW_SIDE: u32 = 320;
/// The longest sides tried, largest first.
const PREVIEW_SIDES: [u32; 7] = [PREVIEW_SIDE, 280, 240, 200, 160, 120, 80];
/// JPEG qualities tried at each size, best first.
const PREVIEW_QUALITIES: [u8; 5] = [80, 70, 60, 50, 40];
/// Bytes of a picture message the preview (as a `data:` URI) and the caption may take
/// together. The engine refuses a message whose preview URI is about 15,000 characters
/// (measured with chat core 7.0: 14,500 sent, 15,500 refused); the official apps keep it
/// to 12,500.
pub const PREVIEW_BUDGET: usize = 12_000;
/// Pictures larger than this are sent without a preview (not decoded).
pub const PREVIEW_MAX_SOURCE: u64 = 64 * 1024 * 1024;

/// A JPEG preview of a picture whose `data:` URI fits `budget` bytes: the largest size (up
/// to [`PREVIEW_SIDE`] on the longest side, the aspect kept) at the best quality that fits;
/// `None` when it cannot be read or nothing fits.
pub fn preview_jpeg(bytes: &[u8], budget: usize) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    for side in PREVIEW_SIDES {
        let small = img.thumbnail(side, side).to_rgb8();
        for q in PREVIEW_QUALITIES {
            let mut out = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q)
                .encode_image(&small)
                .ok()?;
            if data_uri_len(out.len()) <= budget {
                return Some(out);
            }
        }
    }
    None
}

/// The length of a JPEG's `data:image/jpg;base64,…` URI.
pub fn data_uri_len(jpeg: usize) -> usize {
    "data:image/jpg;base64,".len() + jpeg.div_ceil(3) * 4
}
