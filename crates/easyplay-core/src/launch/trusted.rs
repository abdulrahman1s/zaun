//! Running trusted sessions for the launch pipeline and the runtime commands (RT-009,
//! RT-012, RT-008 gate; RUNTIME_MANAGEMENT §4, §6.3).
//!
//! [`TrustedSteps`] is what the orchestrator needs beyond planning: run a Provision or
//! Prepare session to its end with progress, finish an installation (post-failure sweep,
//! manifest rebuild), answer and run the audit gate, and record what happened on the game
//! row. [`LocalTrustedSteps`] implements it over a [`SessionLauncher`], the library database
//! (its own connections; the supervisor and the GUI write concurrently through SQLite's
//! locking) and the runtime store.
//!
//! A store-writing session (Provision, network Prepare) takes the store-wide lock
//! `STORE/.provision.lock` and waits while a Shared-mode game session is open or an audit is
//! pending for a variant that is about to be changed (§4.1 "Concurrency", §6.1).

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustix::fs::{FlockOperation, Mode, OFlags};

use super::planner::PlannedLaunch;
use crate::clock::{Clock, Timestamp};
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::Fs;
use crate::ids::{GameId, RuntimeName, SessionId};
use crate::jobs::CancelToken;
use crate::model::{
    GameFlags, NetworkAccess, RuntimeIsolationMode, Session, SessionKind, SlrVariant,
};
use crate::paths::AppPaths;
use crate::procfs::ProcSource;
use crate::runtime::audit::audit_variant;
use crate::runtime::discovery::detect_slr;
use crate::runtime::integrity::{AuditAction, AuditDue, IntegrityRecords, audit_due, handle_audit};
use crate::runtime::manifest;
use crate::runtime::provision::{
    CACHE_POLL, CacheMeter, PREPARE_TIMEOUT, TrustedEnd, log_shows_interrupted, log_shows_offline,
    rescue_parts, sweep_partial,
};
use crate::runtime::{RuntimeStore, gc};
use crate::session::{SessionLauncher, StopMode};
use crate::storage::{
    AuditState, Db, GamePatch, GameRepo, RuntimeEvent, RuntimeRepo, SessionRepo, SlrInstall,
};
use crate::umu::UmuPhase;

/// Progress of a trusted session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustedProgress {
    /// Waiting for the store lock, a Shared-mode session or an audit.
    Waiting,
    /// umu reported a phase.
    Phase(UmuPhase),
    /// Bytes in umu's cache and the rate (bytes per second).
    Bytes {
        /// Downloaded so far.
        bytes: u64,
        /// Rate.
        rate: u64,
    },
}

/// What changed in the store after an installation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StoreChange {
    /// Partial installs removed.
    pub swept: Vec<PathBuf>,
    /// Variants whose manifest was (re)built.
    pub manifests: Vec<SlrVariant>,
}

/// The trusted-session seam of the orchestrator.
pub trait TrustedSteps: Send + Sync {
    /// Preference `protection.prepare_online`.
    fn prepare_online(&self) -> bool;
    /// Start the session of `planned` and wait for its end. `fast_path`: stop it once umu
    /// reports `Launching` (Provision).
    fn run(
        &self,
        planned: &PlannedLaunch,
        fast_path: bool,
        on: &mut dyn FnMut(TrustedProgress),
        cancel: &CancelToken,
    ) -> Result<TrustedEnd>;
    /// After a Provision or network Prepare: sweep partial installs, rebuild manifests.
    fn after_install(&self) -> Result<StoreChange>;
    /// Whether a launch must audit `variant` first.
    fn audit_due(&self, variant: SlrVariant) -> AuditDue;
    /// Audit `variant` and apply §6.4.
    fn audit(&self, variant: SlrVariant, cancel: &CancelToken) -> Result<AuditAction>;
    /// A Prepare finished: `games.prefix_prepared_with`.
    fn prepared(&self, game: GameId, build: &RuntimeName) -> Result<()>;
    /// A game session was started with `build`: `games.runtime_last_resolved`, last use.
    fn launched(&self, game: GameId, build: &RuntimeName) -> Result<()>;
}

/// The library database as [`IntegrityRecords`].
pub struct DbRecords<'a>(pub &'a mut Db);

impl IntegrityRecords for DbRecords<'_> {
    fn audit_state(&mut self, variant: SlrVariant) -> Result<Option<AuditState>> {
        Ok(self.0.audit_state(variant)?)
    }
    fn record_audit(&mut self, variant: SlrVariant, at: Timestamp, result: &str) -> Result<()> {
        Ok(self.0.record_audit_result(variant, at, result)?)
    }
    fn add_event(&mut self, event: &RuntimeEvent) -> Result<()> {
        self.0.add_runtime_event(event)?;
        Ok(())
    }
    fn forget_install(&mut self, variant: SlrVariant) -> Result<()> {
        self.0.delete_slr_install(variant)?;
        Ok(())
    }
    fn recent_untrusted_sessions(&mut self) -> Result<Vec<Session>> {
        Ok(self.0.session_history(
            None,
            &[SessionKind::Game, SessionKind::Installer, SessionKind::Tool],
            0,
            500,
        )?)
    }
    fn flag_game(&mut self, game: GameId, flag: GameFlags, now: Timestamp) -> Result<()> {
        let Some(g) = self.0.game(game)? else {
            return Ok(());
        };
        let patch = GamePatch {
            flags: Some(g.flags | flag),
            ..GamePatch::default()
        };
        self.0.update_game(game, &patch, now)?;
        Ok(())
    }
}

/// The store-wide provisioning lock (`flock`, released on drop or exit).
#[derive(Debug)]
pub struct StoreLock {
    _fd: OwnedFd,
}

impl StoreLock {
    /// Path of the lock file.
    pub fn path(store: &RuntimeStore) -> PathBuf {
        store.root.join(".provision.lock")
    }

    /// Take the lock without blocking; `Ok(None)` when another process holds it.
    pub fn try_acquire(fs: &dyn Fs, store: &RuntimeStore) -> Result<Option<StoreLock>> {
        fs.create_dir_all(&store.root, 0o700)?;
        let path = Self::path(store);
        let fd = rustix::fs::open(
            &path,
            OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|e| Error::new(ErrorCode::FsWriteDenied).with_source(std::io::Error::from(e)))?;
        match rustix::fs::flock(&fd, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(Some(StoreLock { _fd: fd })),
            Err(rustix::io::Errno::WOULDBLOCK) => Ok(None),
            Err(e) => {
                Err(Error::new(ErrorCode::FsWriteDenied).with_source(std::io::Error::from(e)))
            }
        }
    }
}

/// How long a started session may lack its row before the start counts as failed.
pub const MISSING_ROW: Duration = Duration::from_secs(5);

/// How often the waiter polls the session row.
pub const POLL: Duration = Duration::from_millis(200);

/// The real [`TrustedSteps`].
pub struct LocalTrustedSteps {
    /// Starts and stops supervisors.
    pub launcher: Arc<dyn SessionLauncher>,
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// `/proc` (supervisor liveness).
    pub proc: Arc<dyn ProcSource>,
    /// Locations (database, session logs).
    pub paths: AppPaths,
    /// The runtime store.
    pub store: RuntimeStore,
    /// `protection.prepare_online`.
    pub prepare_online: bool,
}

impl LocalTrustedSteps {
    fn db(&self) -> Result<Db> {
        Ok(Db::open_rw(&self.paths.db_file)?)
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }

    /// Open Shared-mode untrusted sessions (they have the store mounted read-write).
    fn shared_sessions_open(&self, db: &mut Db) -> bool {
        db.open_sessions().is_ok_and(|v| {
            v.iter().any(|s| {
                s.kind.is_untrusted() && s.context.isolation == Some(RuntimeIsolationMode::Shared)
            })
        })
    }

    /// Wait for the store lock, no open Shared session, and no pending audit (which is run).
    fn acquire_store(
        &self,
        on: &mut dyn FnMut(TrustedProgress),
        cancel: &CancelToken,
    ) -> Result<StoreLock> {
        let mut told = false;
        loop {
            if cancel.is_cancelled() {
                return Err(Error::new(ErrorCode::IntTaskFailed).with("reason", "cancelled"));
            }
            let mut db = self.db()?;
            if !self.shared_sessions_open(&mut db)
                && let Some(lock) = StoreLock::try_acquire(&*self.fs, &self.store)?
            {
                drop(db);
                // §6.1: the manifests are rebuilt afterwards; first settle pending audits.
                for v in [SlrVariant::Steamrt3, SlrVariant::Steamrt4] {
                    if self.audit_due(v) == AuditDue::Pending {
                        let action = self.audit(v, cancel)?;
                        if let Some(n) = action.notice() {
                            tracing::warn!(code = n.code.as_str(), "{n}");
                        }
                    }
                }
                return Ok(lock);
            }
            if !told {
                on(TrustedProgress::Waiting);
                told = true;
            }
            self.sleep(Duration::from_secs(1));
        }
    }

    fn log_tail(&self, log_dir: &Path) -> String {
        let path = log_dir.join("game.log");
        let len = self.fs.metadata(&path).map_or(0, |m| m.size);
        let from = len.saturating_sub(64 << 10);
        self.fs
            .read_range(&path, from, 64 << 10)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    }
}

impl TrustedSteps for LocalTrustedSteps {
    fn prepare_online(&self) -> bool {
        self.prepare_online
    }

    fn run(
        &self,
        planned: &PlannedLaunch,
        fast_path: bool,
        on: &mut dyn FnMut(TrustedProgress),
        cancel: &CancelToken,
    ) -> Result<TrustedEnd> {
        let plan = &planned.plan;
        let kind = plan.session.kind;
        let writes_store = kind == SessionKind::Provision
            || (kind == SessionKind::Prepare && plan.context.network == Some(NetworkAccess::Full));
        let _lock = if writes_store {
            Some(self.acquire_store(on, cancel)?)
        } else {
            None
        };
        let handle = self.launcher.start(plan)?;
        let id = handle.session_id;
        let mut ro = Db::open_ro(&self.paths.db_file)?;
        let mut meter = CacheMeter::new(self.store.cache.clone());
        let started = self.clock.mono();
        let mut last_meter = started;
        let mut last_event = 0i64;
        let mut launching_seen = false;
        let mut stop_sent = false;
        let mut fast_path_stopped = false;
        let mut cancelled = false;
        let mut dead_polls = 0u32;
        let downloads = planned
            .sandbox
            .env
            .get("PROTONPATH")
            .is_some_and(|p| !p.starts_with('/'));
        let mut launching_at: Option<crate::clock::MonoInstant> = None;
        let row = loop {
            let now = self.clock.mono();
            for ev in ro.session_events(id).unwrap_or_default() {
                if ev.id <= last_event {
                    continue;
                }
                last_event = ev.id;
                if ev.kind != "phase" {
                    continue;
                }
                if let Ok(p) = serde_json::from_str::<UmuPhase>(&ev.data_json) {
                    if matches!(p, UmuPhase::Launching { .. }) {
                        launching_seen = true;
                    }
                    on(TrustedProgress::Phase(p));
                }
            }
            if writes_store && now.saturating_duration_since(last_meter) >= CACHE_POLL {
                last_meter = now;
                let (bytes, rate) = meter.poll(&*self.fs, now);
                if bytes > 0 {
                    on(TrustedProgress::Bytes { bytes, rate });
                }
            }
            let row = ro.session(id)?;
            match &row {
                Some(s) if !s.is_open() => break s.clone(),
                // A launcher returns only after the supervisor inserted its row.
                None if now.saturating_duration_since(started) > MISSING_ROW => {
                    return Err(Error::new(ErrorCode::SesSupervisorNotStarted)
                        .with("session", id)
                        .with("detail", "no session row"));
                }
                _ => {}
            }
            // The Prepare timeout covers creating the prefix, not a download before it.
            if launching_at.is_none() && (launching_seen || !downloads) {
                launching_at = Some(now);
            }
            let timed_out = kind == SessionKind::Prepare
                && launching_at.is_some_and(|t| now.saturating_duration_since(t) > PREPARE_TIMEOUT);
            let want_stop = (fast_path && launching_seen) || cancel.is_cancelled() || timed_out;
            if want_stop
                && !stop_sent
                && let Some(s) = &row
            {
                fast_path_stopped = fast_path && launching_seen;
                cancelled = cancel.is_cancelled();
                if timed_out {
                    tracing::warn!(session = %id, "the Prepare session timed out");
                }
                match self.launcher.request_stop(s, StopMode::Graceful) {
                    Ok(_) => stop_sent = true,
                    Err(e) => tracing::warn!(error = %e, "stopping the trusted session failed"),
                }
            }
            // A supervisor that died leaves the row open: close it like recovery does.
            if let Some(s) = &row
                && let Some(sup) = crate::session::stop::supervisor_identity(s)
                && !self.proc.is_alive(sup).unwrap_or(true)
            {
                dead_polls += 1;
                if dead_polls > 10 {
                    let mut db = self.db()?;
                    crate::session::recover(
                        &mut db,
                        &*self.proc,
                        &*self.fs,
                        &self.paths,
                        &*self.clock,
                        &crate::session::RecoveryOptions::default(),
                    )?;
                }
            }
            self.sleep(POLL);
        };
        let reason = row
            .end_reason
            .unwrap_or(crate::model::EndReason::Interrupted);
        if writes_store {
            for p in rescue_parts(&*self.fs, &self.store.cache) {
                tracing::info!(part = %p.display(), "partial download kept for the next attempt");
            }
        }
        let failed_download = plan.context.network == Some(NetworkAccess::Full)
            && !matches!(reason, crate::model::EndReason::Exited { code: 0 })
            && !launching_seen;
        let tail = if failed_download {
            self.log_tail(&plan.paths.log_dir)
        } else {
            String::new()
        };
        let network_failure = failed_download && log_shows_offline(&tail);
        let download_interrupted = failed_download && log_shows_interrupted(&tail);
        Ok(TrustedEnd {
            kind,
            reason,
            launching_seen,
            fast_path_stopped,
            cancelled,
            network_failure,
            download_interrupted,
        })
    }

    fn after_install(&self) -> Result<StoreChange> {
        let mut change = StoreChange {
            swept: sweep_partial(&*self.fs, &self.store),
            manifests: Vec::new(),
        };
        let now = self.clock.now();
        let mut db = self.db()?;
        let cancel = CancelToken::new();
        for v in [SlrVariant::Steamrt3, SlrVariant::Steamrt4] {
            let dir = self.store.slr_dir(v);
            let Some(found) = detect_slr(&*self.fs, &dir, v).filter(|s| s.is_complete()) else {
                continue;
            };
            let installed = db.slr_installs()?.into_iter().find(|i| i.variant == v);
            let manifest_ok = installed.as_ref().is_some_and(|i| {
                i.version == found.version.clone().unwrap_or_default()
                    && manifest::load(
                        &*self.fs,
                        &self.store.manifest_file(v),
                        v,
                        i.manifest_tree_sha256.as_ref(),
                    )
                    .is_ok()
            });
            if manifest_ok {
                continue;
            }
            let shim = self.store.umu_shim();
            let m = manifest::build(
                &*self.fs,
                &*self.clock,
                &dir,
                v,
                found.version.clone(),
                Some(&shim),
                now,
                &cancel,
            )?;
            manifest::write(&*self.fs, &self.store.manifests, &m)?;
            let platform_dir = found
                .platform_dir
                .as_ref()
                .and_then(|p| p.file_name())
                .map(PathBuf::from)
                .unwrap_or_default();
            db.put_slr_install(&SlrInstall {
                variant: v,
                version: found.version.clone().unwrap_or_default(),
                path: dir.clone(),
                platform_dir,
                manifest_tree_sha256: crate::model::from_hex32(&m.tree_sha256),
                manifest_built_at: Some(now),
                size_bytes: Some(gc::tree_size(&*self.fs, &*self.clock, &dir)),
            })?;
            db.record_audit_result(v, now, "clean")?;
            db.add_runtime_event(&RuntimeEvent {
                id: 0,
                at: now,
                variant: Some(v),
                kind: if installed.is_some() {
                    "updated".into()
                } else {
                    "provisioned".into()
                },
                summary_json: serde_json::json!({
                    "version": found.version,
                    "entries": m.entries,
                })
                .to_string(),
            })?;
            change.manifests.push(v);
        }
        Ok(change)
    }

    fn audit_due(&self, variant: SlrVariant) -> AuditDue {
        let state = self
            .db()
            .and_then(|mut db| Ok(db.audit_state(variant)?))
            .ok()
            .flatten();
        audit_due(state.as_ref(), self.clock.now())
    }

    fn audit(&self, variant: SlrVariant, cancel: &CancelToken) -> Result<AuditAction> {
        let mut db = self.db()?;
        let stored = db
            .slr_installs()?
            .into_iter()
            .find(|i| i.variant == variant)
            .and_then(|i| i.manifest_tree_sha256);
        let (report, took) = audit_variant(
            &*self.fs,
            &*self.clock,
            &self.store,
            variant,
            stored.as_ref(),
            cancel,
        )?;
        tracing::info!(
            variant = variant.as_str(),
            result = report.as_str(),
            ms = took.as_millis() as u64,
            "runtime audit"
        );
        handle_audit(
            &*self.fs,
            &mut DbRecords(&mut db),
            &self.store,
            variant,
            &report,
            self.clock.now(),
        )
    }

    fn prepared(&self, game: GameId, build: &RuntimeName) -> Result<()> {
        let patch = GamePatch {
            prefix_prepared_with: Some(Some(build.clone())),
            ..GamePatch::default()
        };
        self.db()?.update_game(game, &patch, self.clock.now())?;
        Ok(())
    }

    fn launched(&self, game: GameId, build: &RuntimeName) -> Result<()> {
        let now = self.clock.now();
        let mut db = self.db()?;
        let patch = GamePatch {
            runtime_last_resolved: Some(Some(build.clone())),
            ..GamePatch::default()
        };
        db.update_game(game, &patch, now)?;
        let _ = db.set_runtime_last_used(build, now)?;
        Ok(())
    }
}

/// The sessions currently open in Shared mode with the store mounted (for queueing).
pub fn open_shared_sessions(db: &mut Db) -> Result<Vec<SessionId>> {
    Ok(db
        .open_sessions()?
        .into_iter()
        .filter(|s| {
            s.kind.is_untrusted() && s.context.isolation == Some(RuntimeIsolationMode::Shared)
        })
        .map(|s| s.id)
        .collect())
}
