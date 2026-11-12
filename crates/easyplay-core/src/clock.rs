//! The only source of time.
//!
//! Layer 1. `SystemTime::now()` and `Instant::now()` are forbidden everywhere else by a clippy
//! `disallowed-methods` entry (`clippy.toml`). Wall-clock time ([`Timestamp`]) is for display
//! and ordering only; durations are measured with the monotonic clock ([`MonoInstant`],
//! ARCHITECTURE §13 invariant 9).

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Wall-clock time as Unix milliseconds, UTC. Stored as `INTEGER` (DATA_MODEL §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The Unix epoch.
    pub const EPOCH: Timestamp = Timestamp(0);

    /// A timestamp from Unix milliseconds.
    pub const fn from_millis(ms: i64) -> Self {
        Self(ms)
    }

    /// Unix milliseconds.
    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// `self + d`, saturating at the representable range.
    pub fn saturating_add(self, d: Duration) -> Self {
        let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
        Self(self.0.saturating_add(ms))
    }

    /// `self - d`, saturating at the representable range.
    pub fn saturating_sub(self, d: Duration) -> Self {
        let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
        Self(self.0.saturating_sub(ms))
    }

    /// The wall-clock distance from `earlier` to `self`, or `None` when `earlier` is later.
    /// Use only for display; measure durations with [`MonoInstant`].
    pub fn since(self, earlier: Timestamp) -> Option<Duration> {
        let diff = self.0.checked_sub(earlier.0)?;
        u64::try_from(diff).ok().map(Duration::from_millis)
    }

    /// RFC 3339 text in UTC with millisecond precision, e.g. `2026-11-12T21:04:11.203Z`.
    pub fn to_rfc3339(self) -> String {
        match jiff::Timestamp::from_millisecond(self.0) {
            Ok(ts) => ts.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            Err(_) => format!("@{}ms", self.0),
        }
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

/// A point on the process-local monotonic clock: the time elapsed since an arbitrary origin
/// fixed when the clock was created. Values from different clocks are not comparable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MonoInstant(Duration);

impl MonoInstant {
    /// The origin of the clock.
    pub const ZERO: MonoInstant = MonoInstant(Duration::ZERO);

    /// An instant at `d` after the clock's origin (fakes and tests).
    pub const fn from_duration(d: Duration) -> Self {
        Self(d)
    }

    /// Time since the clock's origin.
    pub const fn as_duration(self) -> Duration {
        self.0
    }

    /// Time from `earlier` to `self`; zero when `earlier` is later.
    pub fn saturating_duration_since(self, earlier: MonoInstant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// `self + d`, or `None` on overflow.
    pub fn checked_add(self, d: Duration) -> Option<Self> {
        self.0.checked_add(d).map(Self)
    }

    /// `self + d`, saturating.
    pub fn saturating_add(self, d: Duration) -> Self {
        Self(self.0.saturating_add(d))
    }
}

/// The source of wall-clock and monotonic time. Implemented by [`SystemClock`] and by
/// `easyplay_testkit::FakeClock`.
pub trait Clock: Send + Sync {
    /// Current wall-clock time.
    fn now(&self) -> Timestamp;
    /// Current monotonic time.
    fn mono(&self) -> MonoInstant;
}

impl<C: Clock + ?Sized> Clock for &C {
    fn now(&self) -> Timestamp {
        (**self).now()
    }
    fn mono(&self) -> MonoInstant {
        (**self).mono()
    }
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn now(&self) -> Timestamp {
        (**self).now()
    }
    fn mono(&self) -> MonoInstant {
        (**self).mono()
    }
}

impl<C: Clock + ?Sized> Clock for Box<C> {
    fn now(&self) -> Timestamp {
        (**self).now()
    }
    fn mono(&self) -> MonoInstant {
        (**self).mono()
    }
}

/// The real clock: `SystemTime` for wall-clock time and `Instant` for monotonic time.
#[derive(Clone, Copy, Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose monotonic origin is now.
    #[allow(clippy::disallowed_methods)] // The one permitted use of `Instant::now`.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    #[allow(clippy::disallowed_methods)] // The one permitted use of `SystemTime::now`.
    fn now(&self) -> Timestamp {
        let ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
            Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
        };
        Timestamp(ms)
    }

    fn mono(&self) -> MonoInstant {
        MonoInstant(self.origin.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_arithmetic() {
        let t = Timestamp::from_millis(1_000);
        assert_eq!(
            t.saturating_add(Duration::from_millis(500)).as_millis(),
            1_500
        );
        assert_eq!(t.saturating_sub(Duration::from_secs(2)).as_millis(), -1_000);
        assert_eq!(
            Timestamp::from_millis(i64::MAX).saturating_add(Duration::from_secs(1)),
            Timestamp::from_millis(i64::MAX)
        );
        assert_eq!(
            Timestamp::from_millis(3_000).since(t),
            Some(Duration::from_secs(2))
        );
        assert_eq!(t.since(Timestamp::from_millis(3_000)), None);
    }

    #[test]
    fn timestamp_rfc3339() {
        let t = Timestamp::from_millis(1_794_517_451_203);
        assert_eq!(t.to_rfc3339(), "2026-11-12T21:04:11.203Z");
        assert_eq!(Timestamp::EPOCH.to_string(), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn timestamp_serde_is_integer() {
        let t = Timestamp::from_millis(1_794_480_000_000);
        assert_eq!(serde_json::to_string(&t).unwrap(), "1794480000000");
        let back: Timestamp = serde_json::from_str("1794480000000").unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn mono_instant_arithmetic() {
        let a = MonoInstant::from_duration(Duration::from_secs(5));
        let b = a.saturating_add(Duration::from_secs(3));
        assert_eq!(b.saturating_duration_since(a), Duration::from_secs(3));
        assert_eq!(a.saturating_duration_since(b), Duration::ZERO);
        assert_eq!(
            a.checked_add(Duration::from_secs(1)),
            Some(MonoInstant::from_duration(Duration::from_secs(6)))
        );
    }

    #[test]
    fn system_clock_is_monotonic_and_plausible() {
        let clock = SystemClock::new();
        let a = clock.mono();
        let b = clock.mono();
        assert!(b >= a);
        // After 2020-01-01.
        assert!(clock.now().as_millis() > 1_577_836_800_000);
        let shared: Arc<dyn Clock> = Arc::new(clock);
        assert!(shared.now().as_millis() > 0);
    }
}
