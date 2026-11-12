//! The supervisor's heartbeat (PROC-005, STATISTICS §4.1): every [`HEARTBEAT_INTERVAL`] the
//! row's `active_ms` (monotonic time since spawn) and `last_heartbeat_at` (wall clock) are
//! updated. A failed write is logged and retried at the next tick, never fatal.
//!
//! The heartbeat is a timer of the supervisor loop rather than a thread: the loop owns the
//! database connection, and a write blocks at most for the busy timeout.

use std::time::Duration;

use crate::clock::{Clock, MonoInstant};
use crate::consts::HEARTBEAT_INTERVAL;
use crate::ids::SessionId;

use super::supervisor::SessionStore;

/// Heartbeat schedule of one session.
#[derive(Clone, Copy, Debug)]
pub struct Heartbeat {
    session: SessionId,
    t0: MonoInstant,
    interval: Duration,
    next: MonoInstant,
    failures: u32,
}

impl Heartbeat {
    /// A heartbeat for `session` whose duration is measured from `t0`.
    pub fn new(session: SessionId, t0: MonoInstant) -> Self {
        Self::with_interval(session, t0, HEARTBEAT_INTERVAL)
    }

    /// With a custom interval (tests).
    pub fn with_interval(session: SessionId, t0: MonoInstant, interval: Duration) -> Self {
        Self {
            session,
            t0,
            interval,
            next: t0.saturating_add(interval),
            failures: 0,
        }
    }

    /// Monotonic time elapsed since spawn.
    pub fn active(&self, now: MonoInstant) -> Duration {
        now.saturating_duration_since(self.t0)
    }

    /// When the next beat is due.
    pub fn next_due(&self) -> MonoInstant {
        self.next
    }

    /// Consecutive failed writes.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// Beat if due. Returns whether a write was attempted.
    pub fn tick(&mut self, store: &mut dyn SessionStore, clock: &dyn Clock) -> bool {
        let now = clock.mono();
        if now < self.next {
            return false;
        }
        self.beat(store, clock);
        // Schedule from now: after a suspend or a long stall the next beat is one interval
        // away, not a burst of missed ones.
        self.next = now.saturating_add(self.interval);
        true
    }

    /// Write one heartbeat now.
    pub fn beat(&mut self, store: &mut dyn SessionStore, clock: &dyn Clock) {
        let active_ms = u64::try_from(self.active(clock.mono()).as_millis()).unwrap_or(u64::MAX);
        match store.heartbeat(self.session, active_ms, clock.now()) {
            Ok(_) => self.failures = 0,
            Err(e) => {
                self.failures += 1;
                tracing::warn!(
                    session = %self.session,
                    failures = self.failures,
                    "heartbeat write failed, retrying at the next tick: {e}"
                );
            }
        }
    }
}
