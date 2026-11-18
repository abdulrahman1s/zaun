//! Closing a session and updating the aggregates (STATISTICS §2, §4.2; LOGIC_SPECS §7).
//!
//! Used by the supervisor at session end, by recovery, and by the `final.json` import. The
//! guarded `UPDATE … WHERE state != 'ended'` in `SessionRepo::close_session` makes a second
//! application a no-op, so any number of closers may race.

use std::time::Duration;

use crate::clock::Timestamp;
use crate::consts::MIN_COUNTED_SESSION;
use crate::ids::SessionId;
use crate::model::{EndReason, Session, SessionKind};
use crate::storage::{CloseOutcome, SessionClose, SessionRepo, StorageResult};

use super::rollup;

/// The inputs of the counted rule that come from preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountRule {
    /// `statistics.enabled` when the session ended.
    pub enabled: bool,
    /// `statistics.min_session_secs`.
    pub min_session: Duration,
}

impl Default for CountRule {
    fn default() -> Self {
        Self {
            enabled: true,
            min_session: MIN_COUNTED_SESSION,
        }
    }
}

impl CountRule {
    /// From `statistics.enabled` and `statistics.min_session_secs`.
    pub fn new(enabled: bool, min_session_secs: u32) -> Self {
        Self {
            enabled,
            min_session: Duration::from_secs(u64::from(min_session_secs)),
        }
    }
}

/// The counted rule of STATISTICS §2: a `Game` session of at least the minimum duration that
/// did not fail to start, ended while statistics were enabled.
pub fn is_counted(kind: SessionKind, duration_ms: u64, reason: EndReason, rule: CountRule) -> bool {
    rule.enabled
        && kind.may_count()
        && !matches!(reason, EndReason::FailedToStart { .. })
        && u128::from(duration_ms) >= rule.min_session.as_millis()
}

/// How a session ended, as observed by its closer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionEnd {
    /// The session.
    pub id: SessionId,
    /// Wall clock at the end.
    pub ended_at: Timestamp,
    /// Monotonic duration (for recovered sessions: `active_ms` at the last heartbeat).
    pub duration_ms: u64,
    /// Why it ended.
    pub end_reason: EndReason,
    /// Raw exit status.
    pub exit_status: Option<i32>,
}

impl SessionEnd {
    /// The end recovery records for a session whose supervisor died (PROCESS_LIFECYCLE §9):
    /// duration = `active_ms` at the last heartbeat, end = that heartbeat (or the start).
    pub fn recovered(session: &Session) -> Self {
        Self {
            id: session.id,
            ended_at: session.last_heartbeat_at.unwrap_or(session.started_at),
            duration_ms: session.active_ms,
            end_reason: EndReason::Interrupted,
            exit_status: None,
        }
    }
}

/// The close of `session` ending as `end`: counted flag and day pieces decided here.
pub fn close_for(session: &Session, end: &SessionEnd, rule: CountRule) -> SessionClose {
    let counted = is_counted(session.kind, end.duration_ms, end.end_reason, rule);
    SessionClose {
        id: end.id,
        ended_at: end.ended_at,
        duration_ms: end.duration_ms,
        end_reason: end.end_reason,
        exit_status: end.exit_status,
        counted,
        pieces: if counted {
            rollup::split(session.started_at, end.duration_ms, session.tz_offset_min)
        } else {
            Vec::new()
        },
    }
}

/// `stats::recorder::apply`: close the session row and, only if this call closed it, update
/// `game_stats` (playtime or the failed-launch counter), `playtime_daily` and
/// `games.last_launched_at` in one transaction. Applying twice changes nothing.
pub fn apply<S: SessionRepo + ?Sized>(
    store: &mut S,
    end: &SessionEnd,
    rule: CountRule,
) -> StorageResult<CloseOutcome> {
    let Some(session) = store.session(end.id)? else {
        return Ok(CloseOutcome::NotFound);
    };
    if !session.is_open() {
        return Ok(CloseOutcome::AlreadyClosed);
    }
    store.close_session(&close_for(&session, end, rule))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn counted_matrix() {
        let on = CountRule::default();
        let off = CountRule {
            enabled: false,
            ..on
        };
        let ok = EndReason::Exited { code: 0 };
        let failed = EndReason::FailedToStart {
            code: ErrorCode::IntTaskFailed,
        };
        let kinds = [
            (SessionKind::Game, true),
            (SessionKind::Installer, false),
            (SessionKind::Tool, false),
            (SessionKind::Prepare, false),
            (SessionKind::Provision, false),
        ];
        for (kind, may) in kinds {
            for (d, long) in [(0, false), (4_999, false), (5_000, true), (3_600_000, true)] {
                for (reason, starts) in [
                    (ok, true),
                    (EndReason::Crashed { signal: 11 }, true),
                    (EndReason::Killed, true),
                    (EndReason::Interrupted, true),
                    (failed, false),
                ] {
                    for (rule, enabled) in [(on, true), (off, false)] {
                        assert_eq!(
                            is_counted(kind, d, reason, rule),
                            may && long && starts && enabled,
                            "{kind:?} {d} {reason:?} {enabled}"
                        );
                    }
                }
            }
        }
        assert!(is_counted(
            SessionKind::Game,
            0,
            ok,
            CountRule::new(true, 0)
        ));
        assert!(!is_counted(
            SessionKind::Game,
            299_999,
            ok,
            CountRule::new(true, 300)
        ));
    }
}
