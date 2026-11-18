//! The design's motion (`chat-calls.html`, motion names): out = 1 − (1 − t)^5 for what moves
//! in, in = t^3 for what moves out, opacity linear; both directions take the same time.
/// The panel, the dim and the closed widgets' fade (140 ms).
pub const FAST: f64 = 0.140;
/// A toast moving in or out (300 ms).
pub const MOVE: f64 = 0.300;
/// Reduced motion: alpha only, this long.
pub const REDUCED: f64 = 0.090;

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(5)
}

pub fn ease_in(t: f32) -> f32 {
    t.clamp(0.0, 1.0).powi(3)
}

/// Progress 0..=1 of something that started at `since` and lasts `len` (seconds).
#[allow(clippy::cast_possible_truncation)]
pub fn progress(now: f64, since: f64, len: f64) -> f32 {
    if len <= 0.0 {
        return 1.0;
    }
    ((now - since) / len).clamp(0.0, 1.0) as f32
}

/// A two-way switch that fades: 0 = off, 1 = on, in between while it moves.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fade {
    on: bool,
    /// When it last flipped (seconds of egui time).
    since: f64,
}

impl Fade {
    pub fn set(&mut self, on: bool, now: f64) {
        if on != self.on {
            self.on = on;
            self.since = now;
        }
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Linear 0..=1 over `len`.
    pub fn value(&self, now: f64, len: f64) -> f32 {
        let p = progress(now, self.since, len);
        if self.on { p } else { 1.0 - p }
    }

    /// Still moving.
    pub fn moving(&self, now: f64, len: f64) -> bool {
        now - self.since < len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_takes_the_same_time_in_and_out() {
        let mut f = Fade::default();
        f.set(true, 1.0);
        assert!((f.value(1.07, FAST) - 0.5).abs() < 0.01);
        assert!((f.value(2.0, FAST) - 1.0).abs() < f32::EPSILON);
        f.set(false, 3.0);
        assert!((f.value(3.07, FAST) - 0.5).abs() < 0.01);
        assert!(!f.moving(3.2, FAST));
    }

    #[test]
    fn the_curves_start_at_zero_and_end_at_one() {
        for f in [ease_out, ease_in] {
            assert!(f(0.0).abs() < f32::EPSILON && (f(1.0) - 1.0).abs() < f32::EPSILON);
        }
        assert!(ease_out(0.2) > 0.5 && ease_in(0.5) < 0.2);
    }
}
