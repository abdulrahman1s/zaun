//! Frame pacing for a share (ADR-0036): pictures come as the game draws them; one is sent
//! at most every 1/fps, the rest are dropped. Never faster than the source: a game that
//! draws less sends what it draws.
use std::time::{Duration, Instant};

/// Lets through at most `fps` pictures a second.
#[derive(Debug)]
pub struct Pacer {
    every: Duration,
    /// When the next picture may go.
    next: Option<Instant>,
}

impl Pacer {
    pub fn new(fps: u32) -> Self {
        Self {
            every: Duration::from_secs(1) / fps.max(1),
            next: None,
        }
    }

    /// A new rate from now on.
    pub fn set_fps(&mut self, fps: u32) {
        *self = Self::new(fps);
    }

    /// Whether a picture taken at `t` goes out.
    pub fn take(&mut self, t: Instant) -> bool {
        // A quarter of a frame early still counts: sources are not exact.
        let slack = self.every / 4;
        if let Some(n) = self.next
            && t + slack < n
        {
            return false;
        }
        // On schedule, keep the beat; late (the source was slow), start from now.
        self.next = Some(match self.next {
            Some(n) if t < n + self.every => n + self.every,
            _ => t + self.every,
        });
        true
    }
}

/// Counts what was sent to report the frame rate actually sent.
#[derive(Debug)]
pub struct RateMeter {
    since: Option<Instant>,
    count: u32,
    /// The last rate measured.
    pub fps: u32,
}

impl RateMeter {
    pub fn new() -> Self {
        Self {
            since: None,
            count: 0,
            fps: 0,
        }
    }

    /// One picture sent at `t`; `Some(rate)` when a two-second window closed.
    pub fn sent(&mut self, t: Instant) -> Option<u32> {
        let since = *self.since.get_or_insert(t);
        self.count += 1;
        let span = t.saturating_duration_since(since);
        if span < Duration::from_secs(2) {
            return None;
        }
        self.fps = (f64::from(self.count - 1) / span.as_secs_f64()).round() as u32;
        self.since = Some(t);
        self.count = 1;
        Some(self.fps)
    }
}

impl Default for RateMeter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pictures let through in one second from a source of `source` a second.
    fn through(source: u32, target: u32) -> u32 {
        let mut p = Pacer::new(target);
        let t0 = crate::clock::now();
        let step = Duration::from_secs(1) / source;
        (0..source).filter(|i| p.take(t0 + step * *i)).count() as u32
    }

    #[test]
    fn pacing_drops_to_the_chosen_rate() {
        assert_eq!(through(60, 30), 30);
        assert_eq!(through(144, 60), 60);
        assert_eq!(through(120, 30), 30);
    }

    #[test]
    fn pacing_never_sends_faster_than_the_game_draws() {
        assert_eq!(through(30, 60), 30);
        assert_eq!(through(45, 60), 45);
    }

    #[test]
    fn the_meter_reports_the_rate_sent_every_two_seconds() {
        let mut m = RateMeter::new();
        let t0 = crate::clock::now();
        let step = Duration::from_millis(1000 / 30);
        let reports: Vec<u32> = (0..=70).filter_map(|i| m.sent(t0 + step * i)).collect();
        assert_eq!(reports.len(), 1);
        assert!((29..=31).contains(&reports[0]), "{reports:?}");
    }
}
