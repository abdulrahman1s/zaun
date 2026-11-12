//! Recovery (PROC-010, PROCESS_LIFECYCLE §9): close session rows whose supervisor is gone.
//! Runs at GUI start, at CLI `launch`/`stop`/`status`, and at the start of every supervisor,
//! inside one `BEGIN IMMEDIATE` transaction. Idempotent: the guarded close lets exactly one
//! closer apply the aggregates.
//!
//! Per open row:
//!
//! 1. the supervisor (PID **and** start time) is alive on this boot → keep;
//! 2. `<session log dir>/final.json` exists and parses → close from the file (the supervisor
//!    finished without the database);
//! 3. the boot ID differs → `Interrupted` (the machine restarted);
//! 4. otherwise → `LauncherTerminated` (the supervisor died).
//!
//! Recovered durations are the last heartbeat's `active_ms`; `ended_at` is the last heartbeat
//! (or the start). Afterwards, session runtime and variable directories that belong to no
//! open row are removed (only when older than a minute, so a supervisor that has not
//! inserted its row yet keeps its plan).
//!
//! Deviation from the order in §9: `final.json` is consulted before the boot-ID comparison,
//! because a record written just before a reboot is more accurate than the heartbeat.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use crate::clock::Clock;
use crate::error::Error;
use crate::fsx::{Fs, safe_remove_tree};
use crate::ids::SessionId;
use crate::model::{EndReason, Session, SessionState};
use crate::paths::AppPaths;
use crate::procfs::ProcSource;
use crate::stats::recorder::is_counted;
use crate::stats::{CountRule, rollup};
use crate::storage::{CloseOutcome, Db, RuntimeRepo, SessionClose, SessionRepo, StorageResult};

use super::stop::supervisor_identity;
use super::supervisor::{FINAL_FILE, FinalRecord};

/// Options of one recovery pass.
#[derive(Clone, Debug)]
pub struct RecoveryOptions {
    /// Sessions whose directories are never removed (the calling supervisor's own).
    pub keep: Vec<SessionId>,
    /// Minimum age of an orphaned directory before it is removed.
    pub stale_dir_age: Duration,
    /// The counted rule.
    pub count_rule: CountRule,
}

impl Default for RecoveryOptions {
    fn default() -> Self {
        Self {
            keep: Vec::new(),
            stale_dir_age: Duration::from_secs(60),
            count_rule: CountRule::default(),
        }
    }
}

/// A row closed by recovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveredSession {
    /// The session.
    pub id: SessionId,
    /// The recorded end.
    pub reason: EndReason,
    /// Recorded duration.
    pub duration_ms: u64,
    /// Whether it counted.
    pub counted: bool,
    /// Closed from `final.json`.
    pub from_final: bool,
}

fn read_final(fs: &dyn Fs, paths: &AppPaths, s: &Session) -> Option<FinalRecord> {
    let bytes = fs
        .read(&paths.session_dir(s.id).join(FINAL_FILE), 1 << 20)
        .ok()?;
    let rec: FinalRecord = serde_json::from_slice(&bytes).ok()?;
    (rec.format == 1 && rec.session == s.id).then_some(rec)
}

/// The recovery decisions over the open rows of `store`; closes what is dead and returns it.
pub fn recover_rows<S: SessionRepo + RuntimeRepo + ?Sized>(
    store: &mut S,
    proc: &dyn ProcSource,
    fs: &dyn Fs,
    paths: &AppPaths,
    rule: CountRule,
) -> StorageResult<Vec<RecoveredSession>> {
    let boot = proc.boot_id().ok();
    let mut out = Vec::new();
    for s in store.open_sessions()? {
        let same_boot = match (&boot, &s.supervisor) {
            (Some(b), Some(v)) => *b == v.boot_id,
            _ => false,
        };
        let alive = same_boot
            && supervisor_identity(&s).is_some_and(|id| proc.is_alive(id).unwrap_or(false));
        if alive {
            continue;
        }
        let (close, audit, from_final) = match read_final(fs, paths, &s) {
            Some(rec) => {
                let pieces = if rec.counted {
                    rollup::split(s.started_at, rec.duration_ms, s.tz_offset_min)
                } else {
                    Vec::new()
                };
                (
                    SessionClose {
                        id: s.id,
                        ended_at: rec.ended_at,
                        duration_ms: rec.duration_ms,
                        end_reason: rec.end_reason,
                        exit_status: rec.exit_status,
                        counted: rec.counted,
                        pieces,
                    },
                    rec.audit,
                    true,
                )
            }
            None => {
                let reason = if same_boot {
                    EndReason::LauncherTerminated
                } else {
                    EndReason::Interrupted
                };
                let counted = s.state != SessionState::Starting
                    && is_counted(s.kind, s.active_ms, reason, rule);
                let pieces = if counted {
                    rollup::split(s.started_at, s.active_ms, s.tz_offset_min)
                } else {
                    Vec::new()
                };
                (
                    SessionClose {
                        id: s.id,
                        ended_at: s.last_heartbeat_at.unwrap_or(s.started_at),
                        duration_ms: s.active_ms,
                        end_reason: reason,
                        exit_status: None,
                        counted,
                        pieces,
                    },
                    None,
                    false,
                )
            }
        };
        if let CloseOutcome::Closed { .. } = store.close_session(&close)? {
            if let Some(v) = audit {
                store.set_audit_pending(v)?;
            }
            tracing::info!(
                session = %s.id,
                reason = close.end_reason.as_str(),
                duration_ms = close.duration_ms,
                from_final,
                "recovered session"
            );
            out.push(RecoveredSession {
                id: s.id,
                reason: close.end_reason,
                duration_ms: close.duration_ms,
                counted: close.counted,
                from_final,
            });
        }
    }
    Ok(out)
}

fn session_dirs(fs: &dyn Fs, root: &Path) -> Vec<(SessionId, i64)> {
    let Ok(entries) = fs.read_dir(root) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|e| {
            let id: SessionId = e.name.to_str()?.parse().ok()?;
            let md = fs.metadata(&root.join(&e.name)).ok()?;
            md.is_dir().then_some((id, md.mtime_ns))
        })
        .collect()
}

/// Remove session runtime and variable directories of sessions that are not open.
pub fn clean_stale_dirs(
    fs: &dyn Fs,
    paths: &AppPaths,
    open: &BTreeSet<SessionId>,
    clock: &dyn Clock,
    min_age: Duration,
) {
    let now_ns = i128::from(clock.now().as_millis()) * 1_000_000;
    for root in [&paths.run_sessions_dir, &paths.session_var_root] {
        for (id, mtime_ns) in session_dirs(fs, root) {
            let age_ns = now_ns - i128::from(mtime_ns);
            if open.contains(&id) || age_ns < min_age.as_nanos() as i128 {
                continue;
            }
            if let Err(e) = safe_remove_tree(fs, root, &root.join(id.to_string())) {
                tracing::warn!("stale session directory not removed: {e}");
            }
        }
    }
}

/// One recovery pass under the database write lock, then directory clean-up.
pub fn recover(
    db: &mut Db,
    proc: &dyn ProcSource,
    fs: &dyn Fs,
    paths: &AppPaths,
    clock: &dyn Clock,
    opts: &RecoveryOptions,
) -> Result<Vec<RecoveredSession>, Error> {
    let recovered = db.transaction(|tx| recover_rows(tx, proc, fs, paths, opts.count_rule))?;
    let mut open: BTreeSet<SessionId> = db.open_sessions()?.into_iter().map(|s| s.id).collect();
    open.extend(opts.keep.iter().copied());
    clean_stale_dirs(fs, paths, &open, clock, opts.stale_dir_age);
    Ok(recovered)
}
