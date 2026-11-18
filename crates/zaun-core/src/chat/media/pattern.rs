//! A test picture for streams (ADR-0036): eight colour bars over the top four fifths and a
//! square that moves along the bottom. The preview shows it on the stage, the media tests
//! send it through the encoder and the network and check what arrives.
use super::frame::VideoFrame;

/// The bars, left to right (studio-range colours, which survive 4:2:0 and lossy coding).
pub const BARS: [[u8; 3]; 8] = [
    [235, 235, 235],
    [235, 235, 16],
    [16, 235, 235],
    [16, 235, 16],
    [235, 16, 235],
    [235, 16, 16],
    [16, 16, 235],
    [16, 16, 16],
];

/// How far a received colour may stray from the bar's and still count.
const TOLERANCE: i32 = 48;

/// Frame `number` of the test picture at this size (the square moves with the number).
#[must_use]
pub fn test_pattern(width: u32, height: u32, number: u64) -> VideoFrame {
    let mut f = VideoFrame::black(width, height);
    let (w, h) = (width as usize, height as usize);
    let band = h * 4 / 5;
    let side = (h / 5).max(1);
    let travel = w.saturating_sub(side).max(1);
    let sx = (number as usize * 8) % travel;
    for (y, row) in f.rgba.chunks_exact_mut(w * 4).enumerate() {
        for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let c = if y < band {
                BARS[x * BARS.len() / w.max(1)]
            } else if (sx..sx + side).contains(&x) {
                [235, 235, 235]
            } else {
                [16, 16, 16]
            };
            p[..3].copy_from_slice(&c);
        }
    }
    f
}

/// Whether `frame` shows the bars of [`test_pattern`] (at any size): each bar's middle,
/// halfway down the bars, is within [`TOLERANCE`] of its colour.
pub fn shows_test_pattern(frame: &VideoFrame) -> bool {
    if frame.width < 16 || frame.height < 10 {
        return false;
    }
    let y = frame.height * 2 / 5;
    BARS.iter().enumerate().all(|(i, bar)| {
        let x = (2 * i as u32 + 1) * frame.width / 16;
        let got = frame.rgb_at(x, y);
        got.iter()
            .zip(bar)
            .all(|(g, b)| (i32::from(*g) - i32::from(*b)).abs() <= TOLERANCE)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_picture_is_recognised_and_a_black_one_is_not() {
        assert!(shows_test_pattern(&test_pattern(320, 180, 7)));
        assert!(shows_test_pattern(&test_pattern(1280, 720, 0)));
        assert!(!shows_test_pattern(&VideoFrame::black(320, 180)));
    }

    #[test]
    fn the_square_moves_from_frame_to_frame() {
        assert_ne!(test_pattern(64, 40, 0), test_pattern(64, 40, 1));
    }
}
