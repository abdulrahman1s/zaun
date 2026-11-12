//! Hand-built image headers for codec limit tests: decompression bombs whose header claims
//! huge dimensions while the file is a few bytes, and a malformed corpus. Built byte by byte
//! so no image library is needed (THREAT_MODEL T-18, task ART-001).

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

fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = ty.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// A PNG claiming `w × h` RGBA pixels with a tiny, valid zlib stream (a bomb when large).
pub fn png_bomb(w: u32, h: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    // zlib stream of a few zero bytes: the decoder would expand it to the claimed size.
    chunk(
        &mut out,
        b"IDAT",
        &[0x78, 0x9C, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01],
    );
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A baseline JPEG header (SOI, SOF0) claiming `w × h` with no scan data.
pub fn jpeg_bomb(w: u16, h: u16) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    // DQT (one table of 64 ones).
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    out.extend_from_slice(&[1u8; 64]);
    // SOF0: precision 8, height, width, 3 components.
    out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
    out.extend_from_slice(&h.to_be_bytes());
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00]);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// A BMP (BITMAPINFOHEADER, 32 bpp) claiming `w × h` with no pixel data.
pub fn bmp_bomb(w: i32, h: i32) -> Vec<u8> {
    let mut out = b"BM".to_vec();
    out.extend_from_slice(&54u32.to_le_bytes()); // file size (a lie)
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel offset
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    out
}

/// A WebP (VP8X) header claiming a `w × h` canvas (each at most 2²⁴).
pub fn webp_bomb(w: u32, h: u32) -> Vec<u8> {
    let mut vp8x = vec![0u8; 10];
    vp8x[4..7].copy_from_slice(&(w - 1).to_le_bytes()[..3]);
    vp8x[7..10].copy_from_slice(&(h - 1).to_le_bytes()[..3]);
    let mut body = b"WEBPVP8X".to_vec();
    body.extend_from_slice(&10u32.to_le_bytes());
    body.extend_from_slice(&vp8x);
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// An ICO directory whose single entry points past the end of the file with a huge size.
pub fn ico_hostile() -> Vec<u8> {
    let mut out = vec![0, 0, 1, 0, 1, 0];
    // width 0 (=256), height 0, colours, reserved, planes, bpp, size, offset
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0]);
    out.extend_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
    out.extend_from_slice(&0x7FFF_FFF0u32.to_le_bytes());
    out
}

/// An ICO whose entry is an embedded PNG bomb.
pub fn ico_with_png_bomb(w: u32, h: u32) -> Vec<u8> {
    let png = png_bomb(w, h);
    let mut out = vec![0, 0, 1, 0, 1, 0];
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0]);
    out.extend_from_slice(&(png.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend_from_slice(&png);
    out
}

/// Inputs that must be refused without a panic: truncated headers, magic bytes followed by
/// garbage, text served as an image.
pub fn malformed_corpus() -> Vec<(&'static str, Vec<u8>)> {
    let png = png_bomb(4, 4);
    vec![
        ("empty", Vec::new()),
        (
            "html",
            b"<!DOCTYPE html><html><body>404</body></html>".to_vec(),
        ),
        ("json", br#"{"success":false}"#.to_vec()),
        ("png magic only", b"\x89PNG\r\n\x1a\n".to_vec()),
        ("png truncated ihdr", png[..20].to_vec()),
        ("png bad crc", {
            let mut p = png.clone();
            p[29] ^= 0xFF;
            p
        }),
        ("png zero size", png_bomb(0, 0)),
        ("jpeg soi only", vec![0xFF, 0xD8, 0xFF]),
        ("jpeg garbage", {
            let mut j = vec![0xFF, 0xD8, 0xFF, 0xE0];
            j.extend((0..200u8).map(|i| i.wrapping_mul(37)));
            j
        }),
        ("jpeg zero size", jpeg_bomb(0, 0)),
        ("webp truncated", b"RIFF\x04\0\0\0WEBP".to_vec()),
        ("bmp negative width", bmp_bomb(-5, 5)),
        ("bmp truncated", b"BM\0\0\0\0\0\0\0\0\0\0\0\0".to_vec()),
        ("ico hostile directory", ico_hostile()),
        ("ico truncated", vec![0, 0, 1, 0, 9, 0, 1, 2, 3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_png_iend_constant() {
        let mut v = Vec::new();
        chunk(&mut v, b"IEND", &[]);
        assert_eq!(&v[8..], &[0xAE, 0x42, 0x60, 0x82]);
    }
}
