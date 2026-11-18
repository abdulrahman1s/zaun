//! Pictures in I420 (8-bit Y, then U and V at half size each way), what the H.264 encoder
//! takes and the decoder gives, and the conversions to and from the 32-bit pixels of the
//! capture and the page (BT.601, studio range, as H.264 assumes without colour information).
use easyplay_core::chat::media::VideoFrame;
use openh264::formats::YUVSource;

/// Where the colours sit in a 32-bit pixel, as the capture delivers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelOrder {
    /// B, G, R, then alpha or padding (PipeWire `BGRx`/`BGRA`).
    Bgrx,
    /// R, G, B, then alpha or padding (`RGBx`/`RGBA`).
    Rgbx,
}

/// One picture in I420. Width and height are even.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct I420 {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl I420 {
    /// A picture of this size (rounded down to even, at least 2×2), its planes allocated.
    pub fn new(width: usize, height: usize) -> Self {
        let (w, h) = (even(width), even(height));
        Self {
            width: w,
            height: h,
            y: vec![16; w * h],
            u: vec![128; w * h / 4],
            v: vec![128; w * h / 4],
        }
    }

    /// Fill from 32-bit pixels of `src_w × src_h` (`stride` bytes a row), scaled to this
    /// picture's size by taking the nearest pixel.
    pub fn read_32bit(
        &mut self,
        src: &[u8],
        src_w: usize,
        src_h: usize,
        stride: usize,
        order: PixelOrder,
    ) {
        let (w, h) = (self.width, self.height);
        if src_w == 0 || src_h == 0 || src.len() < stride * (src_h - 1) + src_w * 4 {
            return;
        }
        let (ri, bi) = match order {
            PixelOrder::Bgrx => (2, 0),
            PixelOrder::Rgbx => (0, 2),
        };
        let at = |x: usize, y: usize| -> [i32; 3] {
            let sx = x * src_w / w;
            let sy = y * src_h / h;
            let i = sy * stride + sx * 4;
            [
                i32::from(src[i + ri]),
                i32::from(src[i + 1]),
                i32::from(src[i + bi]),
            ]
        };
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = at(x, y);
                self.y[y * w + x] = clamp(((66 * r + 129 * g + 25 * b + 128) >> 8) + 16);
            }
        }
        for cy in 0..h / 2 {
            for cx in 0..w / 2 {
                // The average of the 2×2 block the chroma sample covers.
                let mut sum = [0i32; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = at(cx * 2 + dx, cy * 2 + dy);
                    sum.iter_mut().zip(p).for_each(|(s, c)| *s += c);
                }
                let [r, g, b] = sum.map(|s| s / 4);
                let i = cy * (w / 2) + cx;
                self.u[i] = clamp(((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128);
                self.v[i] = clamp(((112 * r - 94 * g - 18 * b + 128) >> 8) + 128);
            }
        }
    }

    /// From a page frame (RGBA), at the frame's own size.
    pub fn from_frame(f: &VideoFrame) -> Self {
        let mut p = Self::new(f.width as usize, f.height as usize);
        p.read_32bit(
            &f.rgba,
            f.width as usize,
            f.height as usize,
            f.width as usize * 4,
            PixelOrder::Rgbx,
        );
        p
    }
}

/// Planes of a picture as the decoder hands them out (each with its row stride).
pub struct Planes<'a> {
    pub width: usize,
    pub height: usize,
    pub y: (&'a [u8], usize),
    pub u: (&'a [u8], usize),
    pub v: (&'a [u8], usize),
}

/// Write `p` into `out` as RGBA (alpha 255), reusing `out`'s buffer when it fits.
pub fn to_frame(p: &Planes<'_>, out: &mut VideoFrame) {
    let (w, h) = (p.width, p.height);
    out.width = w as u32;
    out.height = h as u32;
    out.rgba.resize(w * h * 4, 255);
    for (row, line) in out.rgba.chunks_exact_mut(w * 4).enumerate().take(h) {
        let (yr, cr) = (row * p.y.1, (row / 2) * p.u.1);
        let vr = (row / 2) * p.v.1;
        for (x, px) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let yy = i32::from(p.y.0.get(yr + x).copied().unwrap_or(16)) - 16;
            let u = i32::from(p.u.0.get(cr + x / 2).copied().unwrap_or(128)) - 128;
            let v = i32::from(p.v.0.get(vr + x / 2).copied().unwrap_or(128)) - 128;
            let c = 298 * yy + 128;
            px[0] = clamp((c + 409 * v) >> 8);
            px[1] = clamp((c - 100 * u - 208 * v) >> 8);
            px[2] = clamp((c + 516 * u) >> 8);
            px[3] = 255;
        }
    }
}

impl YUVSource for I420 {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }
}

/// The size to send a `width × height` picture at, no taller than `max_height` (keeping
/// its shape), even both ways.
pub fn fit(width: usize, height: usize, max_height: Option<u32>) -> (usize, usize) {
    let limit = max_height.map_or(height, |m| m as usize);
    if height <= limit || height == 0 {
        return (even(width), even(height));
    }
    (even(width * limit / height), even(limit))
}

fn even(n: usize) -> usize {
    (n & !1).max(2)
}

fn clamp(v: i32) -> u8 {
    // Clamped into range before the cast.
    v.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyplay_core::chat::media::pattern::{BARS, shows_test_pattern, test_pattern};

    fn planes(p: &I420) -> Planes<'_> {
        Planes {
            width: p.width,
            height: p.height,
            y: (&p.y, p.width),
            u: (&p.u, p.width / 2),
            v: (&p.v, p.width / 2),
        }
    }

    #[test]
    fn studio_white_and_black_become_the_studio_lumas_and_neutral_chroma() {
        let mut white = vec![235u8; 4 * 4 * 4];
        white.iter_mut().skip(3).step_by(4).for_each(|a| *a = 0);
        let mut p = I420::new(4, 4);
        p.read_32bit(&white, 4, 4, 16, PixelOrder::Bgrx);
        assert!(p.y.iter().all(|y| (217..=221).contains(y)), "{:?}", p.y);
        assert!(p.u.iter().chain(&p.v).all(|c| (126..=130).contains(c)));
    }

    #[test]
    fn the_test_picture_survives_a_trip_through_i420_in_either_byte_order() {
        let f = test_pattern(320, 180, 3);
        let p = I420::from_frame(&f);
        let mut back = VideoFrame::black(1, 1);
        to_frame(&planes(&p), &mut back);
        assert_eq!((back.width, back.height), (320, 180));
        assert!(shows_test_pattern(&back));
        // The same picture with blue and red swapped, read as BGRx, comes out the same.
        let mut bgr = f.rgba.clone();
        bgr.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|px| px.swap(0, 2));
        let mut q = I420::new(320, 180);
        q.read_32bit(&bgr, 320, 180, 320 * 4, PixelOrder::Bgrx);
        assert_eq!(p, q);
    }

    #[test]
    fn a_large_picture_is_scaled_down_to_the_chosen_height_and_keeps_its_bars() {
        assert_eq!(fit(2560, 1440, Some(720)), (1280, 720));
        assert_eq!(fit(1280, 720, Some(1080)), (1280, 720), "never scaled up");
        assert_eq!(fit(1919, 1081, None), (1918, 1080), "even both ways");
        let f = test_pattern(1920, 1080, 0);
        let mut p = I420::new(1280, 720);
        p.read_32bit(&f.rgba, 1920, 1080, 1920 * 4, PixelOrder::Rgbx);
        let mut back = VideoFrame::black(1, 1);
        to_frame(&planes(&p), &mut back);
        assert!(shows_test_pattern(&back));
        let first = back.rgb_at(1280 / 16, 288);
        assert!(
            first
                .iter()
                .zip(BARS[0])
                .all(|(g, w)| (i32::from(*g) - i32::from(w)).abs() <= 6),
            "{first:?}"
        );
    }

    #[test]
    fn a_short_source_buffer_leaves_the_picture_alone() {
        let mut p = I420::new(4, 4);
        let before = p.clone();
        p.read_32bit(&[0; 8], 4, 4, 16, PixelOrder::Rgbx);
        assert_eq!(p, before);
    }
}
