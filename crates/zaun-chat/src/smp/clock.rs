//! Monotonic time for timeouts, behind a trait so tests can drive it (clippy.toml forbids `Instant::now` elsewhere).
use std::time::Instant;

pub trait Clock: Send + Sync {
    /// Milliseconds since an arbitrary fixed start.
    fn now_ms(&self) -> u64;
}

pub struct SystemClock(Instant);

impl SystemClock {
    #[must_use]
    #[allow(
        clippy::disallowed_methods,
        reason = "the one real monotonic clock of the SMP client"
    )]
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}
