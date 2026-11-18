//! When audits run and what happens on a mismatch (RUNTIME_MANAGEMENT §6.3–6.4, RT-008).
//!
//! [`audit_due`] answers the trigger table of §6.3 from the `runtime_audit` row;
//! [`handle_audit`] applies §6.4: a tampered variant is moved into
//! `STORE/quarantine/<variant>-<ms>` with one rename, its manifest and install row are
//! deleted, a `runtime_events` row records the summary and the suspected games, and those
//! games get [`GameFlags::SUSPECTED_TAMPER`]. The next resolve then reports
//! `NeedsProvision` for the variant.
//!
//! Persistence goes through [`IntegrityRecords`] (implemented on the library database in
//! `launch::trusted`, and in memory by tests).

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use super::audit::{AuditReport, TamperSummary};
use super::store::RuntimeStore;
use crate::clock::Timestamp;
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::Fs;
use crate::ids::GameId;
use crate::model::{GameFlags, RuntimeIsolationMode, Session, SlrVariant};
use crate::storage::{AuditState, RuntimeEvent};

/// A clean audit older than this is repeated before the next launch (§6.3).
pub const AUDIT_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// Whether a launch must wait for an audit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditDue {
    /// No audit needed.
    No,
    /// A Shared-mode session ended since the last audit (`pending = 1`).
    Pending,
    /// The last clean audit is older than [`AUDIT_MAX_AGE`], or there never was one.
    Stale,
}

/// §6.3 for one variant at `now`. `Pending` applies to Shared-mode launches only; `Stale`
/// to every launch.
pub fn audit_due(state: Option<&AuditState>, now: Timestamp) -> AuditDue {
    let Some(s) = state else {
        return AuditDue::Stale;
    };
    if s.pending {
        return AuditDue::Pending;
    }
    match s.last_clean_at {
        Some(at) if now.since(at).is_none_or(|age| age <= AUDIT_MAX_AGE) => AuditDue::No,
        _ => AuditDue::Stale,
    }
}

/// Whether a launch in `mode` waits for an audit that is `due`.
pub fn gate_applies(due: AuditDue, mode: RuntimeIsolationMode) -> bool {
    match due {
        AuditDue::No => false,
        AuditDue::Pending => mode == RuntimeIsolationMode::Shared,
        AuditDue::Stale => true,
    }
}

/// What the store needs to answer.
pub trait IntegrityRecords {
    /// The `runtime_audit` row.
    fn audit_state(&mut self, variant: SlrVariant) -> Result<Option<AuditState>>;
    /// Store an audit result (`clean` / `tampered` / `missing`).
    fn record_audit(&mut self, variant: SlrVariant, at: Timestamp, result: &str) -> Result<()>;
    /// Append an event row.
    fn add_event(&mut self, event: &RuntimeEvent) -> Result<()>;
    /// Forget the install row of a quarantined variant.
    fn forget_install(&mut self, variant: SlrVariant) -> Result<()>;
    /// Untrusted sessions (newest first, bounded) for picking the suspects.
    fn recent_untrusted_sessions(&mut self) -> Result<Vec<Session>>;
    /// Add `flag` to a game (missing games are ignored).
    fn flag_game(&mut self, game: GameId, flag: GameFlags, now: Timestamp) -> Result<()>;
}

/// A game that ran with the variant mounted read-write since the last clean audit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Suspect {
    /// The game.
    pub game: GameId,
    /// Its name at the time.
    pub name: String,
}

/// What [`handle_audit`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditAction {
    /// Nothing to do.
    Clean,
    /// The variant was moved aside; the next launch provisions it again.
    Quarantined {
        /// Variant.
        variant: SlrVariant,
        /// Where it went.
        quarantine: PathBuf,
        /// The differences.
        summary: TamperSummary,
        /// Games that may have caused it (empty: "may be damaged").
        suspects: Vec<Suspect>,
    },
    /// Not installed (nothing to quarantine).
    Missing,
}

impl AuditAction {
    /// The notice for the user: `ZN-RT-020` when a game is suspected, `ZN-RT-021` otherwise.
    pub fn notice(&self) -> Option<Error> {
        let AuditAction::Quarantined {
            variant,
            summary,
            suspects,
            ..
        } = self
        else {
            return None;
        };
        let code = if suspects.is_empty() {
            ErrorCode::RtDamaged
        } else {
            ErrorCode::RtSharedTampered
        };
        let mut e = Error::new(code)
            .with("variant", variant.as_str())
            .with("summary", summary.line());
        if !suspects.is_empty() {
            let names: Vec<&str> = suspects.iter().map(|s| s.name.as_str()).collect();
            e = e.with("games", names.join(", "));
        }
        Some(e)
    }
}

/// The untrusted sessions that had `variant` mounted read-write since `since`.
pub fn suspects(
    sessions: &[Session],
    variant: SlrVariant,
    since: Option<Timestamp>,
) -> Vec<Suspect> {
    let mut out: Vec<Suspect> = Vec::new();
    for s in sessions {
        let shared = s.context.isolation == Some(RuntimeIsolationMode::Shared)
            && s.context.slr_variant == Some(variant)
            && s.kind.is_untrusted();
        let recent = since.is_none_or(|t| s.ended_at.unwrap_or(s.started_at) >= t);
        if let (true, true, Some(game)) = (shared, recent, s.game_id)
            && !out.iter().any(|o| o.game == game)
        {
            out.push(Suspect {
                game,
                name: s.game_name.clone(),
            });
        }
    }
    out
}

#[derive(Serialize)]
struct EventSummary<'a> {
    summary: &'a TamperSummary,
    suspects: &'a [Suspect],
    quarantine: String,
}

/// Apply §6.4 for `report` of `variant`.
pub fn handle_audit(
    fs: &dyn Fs,
    records: &mut dyn IntegrityRecords,
    store: &RuntimeStore,
    variant: SlrVariant,
    report: &AuditReport,
    now: Timestamp,
) -> Result<AuditAction> {
    let summary = match report {
        AuditReport::Clean { notes, .. } => {
            for n in notes {
                tracing::info!(variant = variant.as_str(), "audit note: {n}");
            }
            records.record_audit(variant, now, "clean")?;
            return Ok(AuditAction::Clean);
        }
        AuditReport::Missing => {
            records.record_audit(variant, now, "missing")?;
            return Ok(AuditAction::Missing);
        }
        AuditReport::Tampered(s) => s,
    };
    let since = records.audit_state(variant)?.and_then(|s| s.last_clean_at);
    let sessions = records.recent_untrusted_sessions()?;
    let suspects = suspects(&sessions, variant, since);
    // 1. One rename on the same filesystem; then forget the manifest.
    fs.create_dir_all(&store.quarantine, 0o700)?;
    let quarantine = store.quarantine_dir(variant, now);
    fs.rename(&store.slr_dir(variant), &quarantine)?;
    match fs.remove_file(&store.manifest_file(variant)) {
        Ok(()) => {}
        Err(e) if e.kind == crate::fsx::FsErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    records.forget_install(variant)?;
    records.record_audit(variant, now, "tampered")?;
    // 2. The event row and the flags.
    let json = serde_json::to_string(&EventSummary {
        summary,
        suspects: &suspects,
        quarantine: quarantine.display().to_string(),
    })
    .unwrap_or_else(|_| "{}".into());
    records.add_event(&RuntimeEvent {
        id: 0,
        at: now,
        variant: Some(variant),
        kind: "quarantined".into(),
        summary_json: json,
    })?;
    for s in &suspects {
        records.flag_game(s.game, GameFlags::SUSPECTED_TAMPER, now)?;
    }
    tracing::warn!(
        variant = variant.as_str(),
        summary = summary.line(),
        suspects = suspects.len(),
        quarantine = %quarantine.display(),
        "runtime quarantined"
    );
    Ok(AuditAction::Quarantined {
        variant,
        quarantine,
        summary: summary.clone(),
        suspects,
    })
}
