//! Where the engine's time comes from: a monotonic instant for timers, wall time for items.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub trait Clock: Send {
    fn now(&self) -> Instant;
    /// Milliseconds since the epoch.
    fn wall_ms(&self) -> i64;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[allow(clippy::disallowed_methods, reason = "the engine's one real clock")]
    fn now(&self) -> Instant {
        Instant::now()
    }

    #[allow(clippy::disallowed_methods, reason = "the engine's one real clock")]
    fn wall_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// A clock that moves only when told; clones share one time.
#[derive(Clone, Debug)]
pub struct FakeClock(Arc<Mutex<(Instant, i64)>>);

impl FakeClock {
    #[must_use]
    #[allow(
        clippy::disallowed_methods,
        reason = "only a base point; the fake moves by `advance`"
    )]
    pub fn new(wall_ms: i64) -> Self {
        Self(Arc::new(Mutex::new((Instant::now(), wall_ms))))
    }

    pub fn advance(&self, by: Duration) {
        let mut t = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        t.0 += by;
        t.1 += i64::try_from(by.as_millis()).unwrap_or(i64::MAX);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).0
    }

    fn wall_ms(&self) -> i64 {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).1
    }
}
