//! Wall-clock time for the agent's deadlines, which must survive a restart; a trait so tests can drive it.

/// Milliseconds since the Unix epoch.
pub trait WallClock: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub struct SystemWallClock;

impl WallClock for SystemWallClock {
    #[allow(
        clippy::disallowed_methods,
        reason = "the one wall clock of the agent; deadlines are stored across restarts"
    )]
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}
