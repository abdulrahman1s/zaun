//! Decides when silence means a dead connection (nook smp/client.ts `onTimeout`, `strikes`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Alive,
    /// The timer fired very late (the process was suspended): ping to find out instead of judging.
    Probe,
    Dead,
}

pub struct Liveness {
    max_timeouts: u32,
    late_after: u64,
    strikes: u32,
    seen_rx: u64,
}

impl Liveness {
    #[must_use]
    pub fn new(max_timeouts: u32, late_after_ms: u64, now: u64) -> Self {
        Self {
            max_timeouts,
            late_after: late_after_ms,
            strikes: 0,
            seen_rx: now,
        }
    }

    /// Anything received since the last look clears the strikes.
    pub fn observe_rx(&mut self, last_rx: u64) {
        if last_rx > self.seen_rx {
            self.seen_rx = last_rx;
            self.strikes = 0;
        }
    }

    /// A command sent at `sent_at` with `deadline` got no answer by `now`.
    pub fn on_timeout(&mut self, sent_at: u64, deadline: u64, now: u64) -> Verdict {
        if now.saturating_sub(deadline) > self.late_after {
            return Verdict::Probe;
        }
        if self.seen_rx > sent_at {
            return Verdict::Alive;
        }
        self.strikes += 1;
        if self.strikes >= self.max_timeouts {
            Verdict::Dead
        } else {
            Verdict::Alive
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_silent_timeouts_in_a_row_kill_the_connection() {
        let mut l = Liveness::new(2, 5000, 0);
        assert_eq!(l.on_timeout(30_000, 45_000, 45_000), Verdict::Alive);
        assert_eq!(l.on_timeout(60_000, 75_000, 75_000), Verdict::Dead);
    }

    #[test]
    fn a_timeout_does_not_count_when_the_server_sent_anything_after_the_command() {
        let mut l = Liveness::new(2, 5000, 0);
        for i in 0..6u64 {
            l.observe_rx(i * 30_000 + 10_000);
            assert_eq!(
                l.on_timeout(i * 30_000, i * 30_000 + 15_000, i * 30_000 + 15_000),
                Verdict::Alive
            );
        }
    }

    #[test]
    fn a_timer_firing_very_late_probes_instead_of_judging() {
        let mut l = Liveness::new(2, 5000, 0);
        assert_eq!(l.on_timeout(0, 15_000, 600_000), Verdict::Probe);
        assert_eq!(l.on_timeout(0, 15_000, 600_000), Verdict::Probe);
    }

    #[test]
    fn received_data_clears_the_strikes() {
        let mut l = Liveness::new(2, 5000, 0);
        assert_eq!(l.on_timeout(1000, 2000, 2000), Verdict::Alive);
        l.observe_rx(2500);
        assert_eq!(l.on_timeout(3000, 4000, 4000), Verdict::Alive);
    }
}
