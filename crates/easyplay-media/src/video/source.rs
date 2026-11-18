//! Where a share's pictures come from (ADR-0036), behind traits: the game's window through
//! the desktop's ScreenCast portal and PipeWire (`crate::capture`), or the test picture for
//! tests and the end-to-end run, which cannot show a portal.
use std::time::{Duration, Instant};

use easyplay_core::chat::media::pattern::test_pattern;

use super::i420::{I420, PixelOrder, fit};
use crate::clock::now;

/// What waiting for a picture gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    /// A new picture is in the buffer.
    Frame,
    /// None came in time (the window is hidden, minimised, or nothing changed on it).
    Idle,
    /// The window went away.
    Ended,
    /// The capture broke (for the log).
    Failed(String),
}

/// A share's pictures, read on the share's own thread.
pub trait VideoSource: Send {
    /// Wait up to `timeout` for the next picture and write it into `pic`, scaled to be no
    /// taller than `max_height` (the buffer is resized when the size changes).
    fn next(&mut self, pic: &mut I420, max_height: Option<u32>, timeout: Duration) -> Next;
}

/// What a share asks the capture for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureRequest {
    /// The game's id: the portal's remembered choice is kept per game, so a restored pick is
    /// never another program's window.
    pub game: String,
}

/// Why a capture could not open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The user closed the window picker, or the desktop said no.
    Refused(String),
    /// No portal, no window capture on this desktop, PipeWire failed.
    Failed(String),
}

/// Opens captures. `open` may block for as long as the user takes to choose (the portal's
/// dialog); it runs on the share's thread.
pub trait Capture: Send + Sync {
    fn open(&self, req: &CaptureRequest) -> Result<Box<dyn VideoSource>, CaptureError>;
    /// Whether this capture can work at all (`false`: a build without one).
    fn available(&self) -> bool {
        true
    }
}

/// A build without screen capture: every share fails at once, saying so.
pub struct NoCapture;

impl Capture for NoCapture {
    fn open(&self, _req: &CaptureRequest) -> Result<Box<dyn VideoSource>, CaptureError> {
        Err(CaptureError::Failed(
            "this build cannot capture windows".into(),
        ))
    }

    fn available(&self) -> bool {
        false
    }
}

/// The test picture as a capture: `width × height`, `fps` frames a second, the square moving.
#[derive(Clone, Copy, Debug)]
pub struct PatternCapture {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Capture for PatternCapture {
    fn open(&self, _req: &CaptureRequest) -> Result<Box<dyn VideoSource>, CaptureError> {
        Ok(Box::new(PatternSource {
            spec: *self,
            number: 0,
            due: now(),
        }))
    }
}

struct PatternSource {
    spec: PatternCapture,
    number: u64,
    due: Instant,
}

impl VideoSource for PatternSource {
    fn next(&mut self, pic: &mut I420, max_height: Option<u32>, timeout: Duration) -> Next {
        let t = now();
        let wait = self.due.saturating_duration_since(t);
        if wait > timeout {
            std::thread::sleep(timeout);
            return Next::Idle;
        }
        std::thread::sleep(wait);
        self.due = self.due.max(t) + Duration::from_secs(1) / self.spec.fps.max(1);
        let (w, h) = (self.spec.width as usize, self.spec.height as usize);
        let frame = test_pattern(self.spec.width, self.spec.height, self.number);
        self.number += 1;
        let (tw, th) = fit(w, h, max_height);
        if (pic.width, pic.height) != (tw, th) {
            *pic = I420::new(tw, th);
        }
        pic.read_32bit(&frame.rgba, w, h, w * 4, PixelOrder::Rgbx);
        Next::Frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_capture_paces_its_frames_and_fits_the_chosen_height() {
        let cap = PatternCapture {
            width: 1920,
            height: 1080,
            fps: 50,
        };
        let mut src = cap.open(&CaptureRequest { game: "g".into() }).unwrap();
        let mut pic = I420::default();
        let t = now();
        for _ in 0..5 {
            assert_eq!(
                src.next(&mut pic, Some(720), Duration::from_secs(1)),
                Next::Frame
            );
        }
        assert!(
            t.elapsed() >= Duration::from_millis(70),
            "{:?}",
            t.elapsed()
        );
        assert_eq!((pic.width, pic.height), (1280, 720));
    }

    #[test]
    fn a_build_without_capture_says_so() {
        let r = NoCapture.open(&CaptureRequest { game: "g".into() });
        assert!(matches!(r, Err(CaptureError::Failed(_))));
        assert!(!NoCapture.available());
    }
}
