//! Reconnect delay: 2 s growing by half each failure to 180 s, reset by a session that came up (Env/SQLite.hs `reconnectInterval`).
use std::time::Duration;

pub struct Backoff {
    min: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    #[must_use]
    pub fn new(min: Duration, max: Duration) -> Self {
        Self {
            min,
            max,
            next: min,
        }
    }

    /// The wait before the next attempt; the following one is half as long again.
    pub fn next_delay(&mut self) -> Duration {
        let now = self.next;
        self.next = (self.next + self.next / 2).min(self.max);
        now
    }

    pub fn reset(&mut self) {
        self.next = self.min;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_by_half_up_to_its_cap_and_resets() {
        let mut b = Backoff::new(Duration::from_secs(2), Duration::from_secs(5));
        let secs: Vec<_> = (0..4).map(|_| b.next_delay().as_millis()).collect();
        assert_eq!(secs, [2000, 3000, 4500, 5000]);
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_secs(2));
    }
}
