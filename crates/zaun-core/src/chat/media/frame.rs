//! The picture of a watched stream (ADR-0036): decoded frames handed from the media's decoder
//! thread to the page through one slot that keeps only the latest frame.
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// One decoded picture: `width × height` pixels, RGBA row by row, alpha always 255.
#[derive(Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl VideoFrame {
    /// A black frame of this size.
    #[must_use]
    pub fn black(width: u32, height: u32) -> Self {
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        rgba.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|p| p[3] = 255);
        Self {
            width,
            height,
            rgba,
        }
    }

    /// The pixel at (`x`, `y`) as `[r, g, b]`.
    pub fn rgb_at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        match self.rgba.get(i..i + 3) {
            Some(p) => [p[0], p[1], p[2]],
            None => [0; 3],
        }
    }
}

impl fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VideoFrame({}×{})", self.width, self.height)
    }
}

/// Called after a new frame arrived (the UI's repaint request).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Slot {
    latest: Mutex<Option<Arc<VideoFrame>>>,
    /// Counts frames published; 0 = none yet.
    number: AtomicU64,
    waker: Mutex<Option<Waker>>,
}

/// The latest frame of the watched stream. The decoder publishes, the page takes what is
/// newer than what it drew; a frame not drawn before the next one arrives is dropped, so
/// nothing queues up. Clones share the slot.
#[derive(Clone, Default)]
pub struct FrameSlot(Arc<Slot>);

impl FrameSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the latest frame and wake the page. Returns the replaced frame when nobody
    /// holds it any more, so the decoder can reuse its buffer.
    pub fn publish(&self, frame: VideoFrame) -> Option<VideoFrame> {
        let old = self
            .0
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(Arc::new(frame));
        self.0.number.fetch_add(1, Ordering::AcqRel);
        let waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(w) = waker {
            w();
        }
        old.and_then(|f| Arc::try_unwrap(f).ok())
    }

    /// The latest frame and its number when it is newer than frame number `seen`.
    pub fn newer_than(&self, seen: u64) -> Option<(u64, Arc<VideoFrame>)> {
        let n = self.0.number.load(Ordering::Acquire);
        if n <= seen {
            return None;
        }
        let latest = self
            .0
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        Some((n, latest))
    }

    /// Frames published so far.
    pub fn number(&self) -> u64 {
        self.0.number.load(Ordering::Acquire)
    }

    /// Forget the picture (watching stopped). The number keeps counting.
    pub fn clear(&self) {
        *self.0.latest.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Whether there is a picture to draw.
    pub fn has_frame(&self) -> bool {
        self.0
            .latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Call `waker` after each new frame (the page asks to be drawn again).
    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.0.waker.lock().unwrap_or_else(PoisonError::into_inner) = waker;
    }

    pub fn has_waker(&self) -> bool {
        self.0
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Whether both are the same slot.
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for FrameSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FrameSlot(#{})", self.number())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_frame_not_drawn_before_the_next_one_is_dropped_and_its_buffer_comes_back() {
        let slot = FrameSlot::new();
        assert!(slot.newer_than(0).is_none());
        assert!(slot.publish(VideoFrame::black(2, 2)).is_none());
        let back = slot.publish(VideoFrame::black(4, 4));
        assert_eq!(
            back.map(|f| f.width),
            Some(2),
            "the stale frame's buffer is reused"
        );
        let (n, f) = slot.newer_than(0).unwrap();
        assert_eq!((n, f.width), (2, 4), "only the latest frame is there");
        assert!(
            slot.newer_than(n).is_none(),
            "nothing newer than what was drawn"
        );
    }

    #[test]
    fn a_frame_the_page_still_holds_is_not_handed_back() {
        let slot = FrameSlot::new();
        slot.publish(VideoFrame::black(2, 2));
        let (_, held) = slot.newer_than(0).unwrap();
        assert!(slot.publish(VideoFrame::black(2, 2)).is_none());
        assert_eq!(held.width, 2);
    }

    #[test]
    fn each_new_frame_wakes_the_page() {
        let slot = FrameSlot::new();
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        slot.set_waker(Some(Arc::new(move || {
            w.fetch_add(1, Ordering::SeqCst);
        })));
        slot.publish(VideoFrame::black(1, 1));
        slot.publish(VideoFrame::black(1, 1));
        assert_eq!(woken.load(Ordering::SeqCst), 2);
        slot.clear();
        assert!(!slot.has_frame());
        assert_eq!(slot.number(), 2);
    }
}
