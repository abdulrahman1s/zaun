//! `FakeClock`: manual time with independent wall and monotonic clocks.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

use zaun_core::clock::{Clock, MonoInstant, Timestamp};

/// A clock that only moves when told to. Wall-clock time can jump (forwards or backwards,
/// like an NTP correction or suspend) without affecting monotonic time.
#[derive(Debug)]
pub struct FakeClock {
    wall_ms: AtomicI64,
    mono_ns: AtomicU64,
}

impl FakeClock {
    /// Wall clock at `start`, monotonic clock at zero.
    pub fn new(start: Timestamp) -> Self {
        Self {
            wall_ms: AtomicI64::new(start.as_millis()),
            mono_ns: AtomicU64::new(0),
        }
    }

    /// Wall clock at `ms` Unix milliseconds.
    pub fn at_millis(ms: i64) -> Self {
        Self::new(Timestamp::from_millis(ms))
    }

    /// 2026-11-12T00:00:00Z, the specification baseline.
    pub fn baseline() -> Self {
        Self::at_millis(1_794_441_600_000)
    }

    /// Advance both clocks by `d`.
    pub fn advance(&self, d: Duration) {
        self.advance_wall(d);
        self.advance_mono(d);
    }

    /// Advance only the wall clock.
    pub fn advance_wall(&self, d: Duration) {
        let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
        self.wall_ms.fetch_add(ms, Ordering::SeqCst);
    }

    /// Advance only the monotonic clock.
    pub fn advance_mono(&self, d: Duration) {
        let ns = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        self.mono_ns.fetch_add(ns, Ordering::SeqCst);
    }

    /// Set the wall clock (may go backwards).
    pub fn set_wall(&self, t: Timestamp) {
        self.wall_ms.store(t.as_millis(), Ordering::SeqCst);
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::baseline()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.wall_ms.load(Ordering::SeqCst))
    }

    fn mono(&self) -> MonoInstant {
        MonoInstant::from_duration(Duration::from_nanos(self.mono_ns.load(Ordering::SeqCst)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_clocks() {
        let c = FakeClock::at_millis(1_000);
        assert_eq!(c.now().as_millis(), 1_000);
        assert_eq!(c.mono(), MonoInstant::ZERO);
        c.advance(Duration::from_secs(2));
        assert_eq!(c.now().as_millis(), 3_000);
        assert_eq!(c.mono().as_duration(), Duration::from_secs(2));
        c.set_wall(Timestamp::from_millis(0));
        assert_eq!(c.now().as_millis(), 0);
        assert_eq!(c.mono().as_duration(), Duration::from_secs(2));
        c.advance_mono(Duration::from_millis(5));
        assert_eq!(c.now().as_millis(), 0);
        assert_eq!(c.mono().as_duration(), Duration::from_millis(2005));
        assert_eq!(
            FakeClock::baseline().now().to_rfc3339(),
            "2026-11-12T00:00:00.000Z"
        );
    }
}
