//! The wait between attempts to send one message (RetryInterval.hs `withRetryInterval`, AGPL-3.0-only).
use crate::agent::client::{RetryInterval, ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryMode {
    Fast,
    /// After QUOTA, until `A_QCONT` says there is room.
    Slow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Retry {
    pub mode: RetryMode,
    delay: i64,
    elapsed: i64,
}

impl Retry {
    pub(crate) fn new(mode: RetryMode, ri: &RetryInterval) -> Self {
        Self {
            mode,
            delay: ms(ri.initial),
            elapsed: 0,
        }
    }

    /// The wait before the next attempt. It stays at `initial` until `increase_after` has been waited in total, then grows by half up to `max`.
    pub(crate) fn next_delay(&mut self, ri: &RetryInterval) -> i64 {
        let d = self.delay;
        self.elapsed = self.elapsed.saturating_add(d);
        if self.elapsed >= ms(ri.increase_after) {
            self.delay = (self.delay.saturating_mul(3) / 2).min(ms(ri.max));
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::DeliveryConfig;

    #[test]
    fn fast_retry_waits_two_seconds_for_ten_then_grows_by_half_to_two_minutes() {
        let ri = DeliveryConfig::default().fast;
        let mut r = Retry::new(RetryMode::Fast, &ri);
        let waits: Vec<i64> = (0..8).map(|_| r.next_delay(&ri) / 1000).collect();
        assert_eq!(waits, [2, 2, 2, 2, 2, 3, 4, 6]);
        let last = (0..40).map(|_| r.next_delay(&ri)).last().unwrap();
        assert_eq!(last, 120_000);
    }

    #[test]
    fn slow_retry_starts_at_five_minutes_and_stops_at_six_hours() {
        let ri = DeliveryConfig::default().slow;
        let mut r = Retry::new(RetryMode::Slow, &ri);
        assert_eq!(r.next_delay(&ri), 300_000);
        assert_eq!(r.next_delay(&ri), 450_000);
        let last = (0..40).map(|_| r.next_delay(&ri)).last().unwrap();
        assert_eq!(last, 6 * 3600 * 1000);
    }
}
