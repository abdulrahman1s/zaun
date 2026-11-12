//! End classification (PROC-007): the ordered table of PROCESS_LIFECYCLE §7. Pure.

use std::time::Duration;

use crate::consts::MIN_COUNTED_SESSION;
use crate::error::ErrorCode;
use crate::model::{EndReason, SessionKind};
use crate::stats::{CountRule, is_counted};

/// Who asked the session to stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopOrigin {
    /// The UI or CLI (a `stop-request` file preceded the signal).
    User,
    /// The supervisor got `SIGTERM`/`SIGHUP`/`SIGINT` without a request file.
    System,
}

/// Everything the classification reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndFacts {
    /// Session kind.
    pub kind: SessionKind,
    /// Spawning failed with this code (no sandbox ran).
    pub spawn_failed: Option<ErrorCode>,
    /// bwrap reported `child-pid`.
    pub child_pid_seen: bool,
    /// A stop was requested.
    pub stop: Option<StopOrigin>,
    /// The force stage was needed (or requested).
    pub forced: bool,
    /// The log classifier's failure (umu, container or inner/outer bwrap error).
    pub log_failure: Option<ErrorCode>,
    /// Exit status of the main program (exit code, or 128 + signal).
    pub exit_status: Option<i32>,
    /// `Running` was reached.
    pub running_reached: bool,
    /// Monotonic duration.
    pub duration: Duration,
}

/// The classification: the end reason and whether the session may count (before the
/// statistics rule of STATISTICS §2 is applied).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndClass {
    /// Why it ended.
    pub reason: EndReason,
    /// The table's `counted` column.
    pub countable: bool,
}

const CRASH_SIGNALS: [i32; 5] = [11, 6, 4, 7, 8];

/// Evaluate the table; the first matching row wins.
pub fn classify_end(f: &EndFacts) -> EndClass {
    let class = |reason, countable| EndClass { reason, countable };
    // 1. Spawn failed, or bwrap exited before reporting its child.
    if let Some(code) = f.spawn_failed {
        return class(EndReason::FailedToStart { code }, false);
    }
    if !f.child_pid_seen {
        let code = f.log_failure.unwrap_or(ErrorCode::SbxSetupFailed);
        return class(EndReason::FailedToStart { code }, false);
    }
    // 2. Stop requested by the user.
    if f.stop == Some(StopOrigin::User) {
        let reason = if f.forced {
            EndReason::Killed
        } else {
            EndReason::StoppedByUser
        };
        return class(reason, f.running_reached);
    }
    // 3. Shutdown by a signal.
    if f.stop == Some(StopOrigin::System) {
        return class(EndReason::SystemShutdown, f.running_reached);
    }
    // 4. A recognised umu/container failure before Running.
    if let Some(code) = f.log_failure
        && !f.running_reached
    {
        return class(EndReason::FailedToStart { code }, false);
    }
    let signal = f
        .exit_status
        .filter(|s| *s > 128 && *s < 128 + 65)
        .map(|s| s - 128);
    // 5. Crash signals.
    if let Some(signal) = signal.filter(|s| CRASH_SIGNALS.contains(s)) {
        return class(EndReason::Crashed { signal }, f.running_reached);
    }
    // 6. Killed without a request.
    if signal.is_some_and(|s| s == 9 || s == 15) {
        return class(EndReason::Killed, f.running_reached);
    }
    // 7. Short failing run that never reached Running.
    if !f.running_reached && f.duration < MIN_COUNTED_SESSION && f.exit_status != Some(0) {
        return class(
            EndReason::FailedToStart {
                code: ErrorCode::UmuProgramDidNotStart,
            },
            false,
        );
    }
    // 8. Otherwise.
    class(
        EndReason::Exited {
            code: f.exit_status.unwrap_or(-1),
        },
        true,
    )
}

/// The final `counted` flag: the table's column and the statistics rule.
pub fn counted(f: &EndFacts, class: &EndClass, rule: CountRule) -> bool {
    let ms = u64::try_from(f.duration.as_millis()).unwrap_or(u64::MAX);
    class.countable && is_counted(f.kind, ms, class.reason, rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> EndFacts {
        EndFacts {
            kind: SessionKind::Game,
            spawn_failed: None,
            child_pid_seen: true,
            stop: None,
            forced: false,
            log_failure: None,
            exit_status: Some(0),
            running_reached: true,
            duration: Duration::from_secs(60),
        }
    }

    fn reason(f: EndFacts) -> (EndReason, bool) {
        let c = classify_end(&f);
        (c.reason, counted(&f, &c, CountRule::default()))
    }

    #[test]
    fn every_row() {
        let fail = |code| EndReason::FailedToStart { code };
        // 1
        assert_eq!(
            reason(EndFacts {
                spawn_failed: Some(ErrorCode::HostBwrapNotFound),
                ..base()
            }),
            (fail(ErrorCode::HostBwrapNotFound), false)
        );
        assert_eq!(
            reason(EndFacts {
                child_pid_seen: false,
                ..base()
            }),
            (fail(ErrorCode::SbxSetupFailed), false)
        );
        // 2
        assert_eq!(
            reason(EndFacts {
                stop: Some(StopOrigin::User),
                exit_status: Some(143),
                ..base()
            }),
            (EndReason::StoppedByUser, true)
        );
        assert_eq!(
            reason(EndFacts {
                stop: Some(StopOrigin::User),
                forced: true,
                exit_status: Some(137),
                ..base()
            }),
            (EndReason::Killed, true)
        );
        assert_eq!(
            reason(EndFacts {
                stop: Some(StopOrigin::User),
                running_reached: false,
                ..base()
            }),
            (EndReason::StoppedByUser, false)
        );
        // 3
        assert_eq!(
            reason(EndFacts {
                stop: Some(StopOrigin::System),
                ..base()
            }),
            (EndReason::SystemShutdown, true)
        );
        // 4
        assert_eq!(
            reason(EndFacts {
                log_failure: Some(ErrorCode::UmuInternalError),
                running_reached: false,
                exit_status: Some(1),
                ..base()
            }),
            (fail(ErrorCode::UmuInternalError), false)
        );
        // 4 does not apply once Running was reached.
        assert_eq!(
            reason(EndFacts {
                log_failure: Some(ErrorCode::UmuInternalError),
                exit_status: Some(1),
                ..base()
            }),
            (EndReason::Exited { code: 1 }, true)
        );
        // 5
        assert_eq!(
            reason(EndFacts {
                exit_status: Some(139),
                ..base()
            }),
            (EndReason::Crashed { signal: 11 }, true)
        );
        assert_eq!(
            reason(EndFacts {
                exit_status: Some(134),
                running_reached: false,
                ..base()
            }),
            (EndReason::Crashed { signal: 6 }, false)
        );
        // 6
        assert_eq!(
            reason(EndFacts {
                exit_status: Some(137),
                ..base()
            }),
            (EndReason::Killed, true)
        );
        // 7
        assert_eq!(
            reason(EndFacts {
                exit_status: Some(3),
                running_reached: false,
                duration: Duration::from_secs(2),
                ..base()
            }),
            (fail(ErrorCode::UmuProgramDidNotStart), false)
        );
        // 8
        assert_eq!(
            reason(EndFacts {
                exit_status: Some(3),
                running_reached: false,
                ..base()
            }),
            (EndReason::Exited { code: 3 }, true)
        );
        assert_eq!(
            reason(EndFacts {
                running_reached: false,
                duration: Duration::from_secs(2),
                ..base()
            }),
            (EndReason::Exited { code: 0 }, false),
            "too short to count"
        );
        assert_eq!(
            reason(EndFacts {
                kind: SessionKind::Prepare,
                ..base()
            }),
            (EndReason::Exited { code: 0 }, false)
        );
    }

    #[test]
    fn precedence() {
        // A spawn failure beats a stop request; a user stop beats a crash status; a system
        // stop beats a log failure; a log failure beats a crash before Running.
        assert!(matches!(
            classify_end(&EndFacts {
                spawn_failed: Some(ErrorCode::SbxSetupFailed),
                stop: Some(StopOrigin::User),
                ..base()
            })
            .reason,
            EndReason::FailedToStart { .. }
        ));
        assert_eq!(
            classify_end(&EndFacts {
                stop: Some(StopOrigin::User),
                exit_status: Some(139),
                ..base()
            })
            .reason,
            EndReason::StoppedByUser
        );
        assert_eq!(
            classify_end(&EndFacts {
                stop: Some(StopOrigin::System),
                log_failure: Some(ErrorCode::UmuInternalError),
                running_reached: false,
                ..base()
            })
            .reason,
            EndReason::SystemShutdown
        );
        assert!(matches!(
            classify_end(&EndFacts {
                log_failure: Some(ErrorCode::UmuContainerFailed),
                exit_status: Some(139),
                running_reached: false,
                ..base()
            })
            .reason,
            EndReason::FailedToStart {
                code: ErrorCode::UmuContainerFailed
            }
        ));
        // A crash beats the short-run rule.
        assert_eq!(
            classify_end(&EndFacts {
                exit_status: Some(139),
                running_reached: false,
                duration: Duration::from_secs(1),
                ..base()
            })
            .reason,
            EndReason::Crashed { signal: 11 }
        );
    }
}
