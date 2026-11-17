//! Opening a damaged or not updatable library, and leaving recovery mode (ERR-004;
//! PERSISTENCE §8, LOGIC_SPECS §15, ERROR_HANDLING §4.12–4.13 and §5).
//!
//! [`open_library`] runs at start-up before anything reads the library:
//!
//! 1. A `quick_check` of the file (read-only). A damaged file is salvaged into a new
//!    database next to it. When every game survived, the salvaged file is swapped in (the
//!    damaged one is kept as `backups/corrupt-<ts>.db`) and start-up continues with a
//!    "repaired" notice. Otherwise the service starts in **recovery mode** on an empty
//!    in-memory library and offers the choices of PERSISTENCE §8: restore a backup, use the
//!    repaired library, start empty, quit ([`resolve`]).
//! 2. The migration. A failed step leaves the previous version restored: the library opens
//!    read-only (recovery mode, `EP-DB-013`). A library from a newer EasyPlay is not touched:
//!    recovery mode on an empty library (`EP-DB-010`).
//!
//! In recovery mode library writes and launches are refused with the recovery error
//! (`AppService::refuse_in_recovery`); preferences, the Doctor, reports and stopping running
//! sessions keep working. Running sessions are never affected: their supervisors write
//! `final.json` if the database rejects their writes. `AppService::resolve_database` carries
//! out the user's choice and reopens the library.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use super::app_service::{AppService, lib_deps};
use super::command::DatabaseChoice;
use super::event::{Notice, RecoveredHow};
use super::snapshot::{RecoveryReason, RecoveryState};
use crate::clock::{Clock, Timestamp};
use crate::error::catalog::{self, DescribeCtx};
use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::library::Library;
use crate::paths::AppPaths;
use crate::storage::integrity::{self, IntegrityReport};
use crate::storage::{Db, LibraryFiles, MigrateOptions, MigrationOutcome, StorageError, backup};

/// What [`open_library`] produced.
pub struct Opened {
    /// The database the library runs on (an in-memory one while a decision is pending).
    pub db: Db,
    /// Recovery mode, when the library cannot be written.
    pub recovery: Option<RecoveryState>,
    /// The salvaged file waiting for "Use the repaired library".
    pub repaired: Option<PathBuf>,
    /// Shown after start: the library was repaired automatically.
    pub repaired_notice: Option<(RecoveredHow, Option<u64>)>,
}

fn user(code: ErrorCode, err: Error) -> crate::error::UserError {
    let mut u = catalog::describe(&err, &DescribeCtx::plain());
    u.code = code;
    u
}

fn empty_db() -> Result<Db, Error> {
    let mut db = Db::open_in_memory()?;
    db.migrate(&MigrateOptions::default())?;
    Ok(db)
}

/// Valid backups, newest first: (path, taken at, games).
pub fn usable_backups(fs: &dyn Fs, dir: &Path) -> Vec<(PathBuf, Timestamp, u64)> {
    let Ok(list) = backup::list(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, Timestamp, u64)> = list
        .into_iter()
        .filter(|e| e.kind.is_some())
        .filter_map(|e| {
            let info = backup::validate(&e.path).ok()?;
            let at = fs
                .metadata(&e.path)
                .map(|m| Timestamp::from_millis(m.mtime_ns / 1_000_000))
                .unwrap_or(Timestamp::EPOCH);
            Some((e.path, at, info.games))
        })
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.1));
    out
}

/// Names of games in the damaged file that salvage did not recover (best effort).
fn missing_games(damaged: &Path, repaired: &Path) -> Vec<String> {
    use crate::storage::GameRepo;
    let names = |p: &Path| -> Vec<(crate::ids::GameId, String)> {
        crate::storage::ReadDb::open(p)
            .ok()
            .and_then(|mut db| db.load_library().ok())
            .map(|rows| rows.games.into_iter().map(|g| (g.id, g.name)).collect())
            .unwrap_or_default()
    };
    let kept: Vec<_> = names(repaired).into_iter().map(|(id, _)| id).collect();
    names(damaged)
        .into_iter()
        .filter(|(id, _)| !kept.contains(id))
        .map(|(_, n)| n)
        .take(50)
        .collect()
}

/// Step 1 and 2 of the module docs.
pub fn open_library(fs: &dyn Fs, paths: &AppPaths, clock: &dyn Clock) -> Result<Opened, Error> {
    fs.create_dir_all(&paths.data_dir, 0o700)?;
    let files = LibraryFiles::from_paths(paths);
    let now = clock.now();
    let mut repaired_notice = None;
    if fs.exists(&paths.db_file) {
        let damaged = match integrity::check_file(&paths.db_file) {
            Ok(IntegrityReport::Ok) => None,
            Ok(IntegrityReport::Problems(p)) => Some(p.join("; ")),
            Err(StorageError::NewerSchema { .. }) => None,
            Err(e) if e.is_corruption() || matches!(e, StorageError::Foreign { .. }) => {
                Some(e.to_string())
            }
            Err(e) => return Err(e.into()),
        };
        if let Some(problem) = damaged {
            tracing::error!(code = "EP-DB-001", "library damaged: {problem}");
            let out = paths
                .data_dir
                .join(format!("library.salvage-{}.db", now.as_millis()));
            let report = integrity::salvage(&paths.db_file, &out, now);
            match report {
                Ok(r) if r.all_games_present => {
                    integrity::swap_in(&files, &out, now)?;
                    repaired_notice = Some((RecoveredHow::Repaired, r.history_rows_lost));
                    tracing::info!(games = r.games_recovered, "library repaired");
                }
                other => {
                    let (repaired, repaired_games) = match &other {
                        Ok(r) => (Some(out.clone()), Some(r.games_recovered)),
                        Err(e) => {
                            tracing::warn!("salvage failed: {e}");
                            let _ = fs.remove_file(&out);
                            (None, None)
                        }
                    };
                    let missing = repaired
                        .as_deref()
                        .map(|r| missing_games(&paths.db_file, r))
                        .unwrap_or_default();
                    let err = err!(DbCorrupt, detail = problem);
                    return Ok(Opened {
                        db: empty_db()?,
                        recovery: Some(RecoveryState {
                            reason: RecoveryReason::Damaged {
                                backups: usable_backups(fs, &paths.backups_dir),
                                repaired_games,
                                missing_games: missing,
                            },
                            error: user(ErrorCode::DbCorrupt, err),
                        }),
                        repaired,
                        repaired_notice: None,
                    });
                }
            }
        }
    }
    let mut db = Db::open_rw(&paths.db_file)?;
    let outcome = db.migrate(&MigrateOptions {
        backups_dir: Some(paths.backups_dir.clone()),
        now,
    })?;
    let recovery = match outcome {
        MigrationOutcome::Ready | MigrationOutcome::Migrated { .. } => None,
        MigrationOutcome::Failed { step, error, .. } => {
            tracing::error!(code = "EP-DB-013", step, "migration failed: {error}");
            Some(RecoveryState {
                reason: RecoveryReason::MigrationFailed { step },
                error: user(
                    ErrorCode::DbMigrationFailed,
                    err!(DbMigrationFailed, step = step, detail = error),
                ),
            })
        }
        MigrationOutcome::RefusedNewer { found, supported } => {
            db = empty_db()?;
            Some(RecoveryState {
                reason: RecoveryReason::NewerVersion,
                error: user(
                    ErrorCode::DbNewerVersion,
                    err!(DbNewerVersion, version = found, supported = supported),
                ),
            })
        }
    };
    Ok(Opened {
        db,
        recovery,
        repaired: None,
        repaired_notice,
    })
}

/// Carry out the user's choice for a damaged library; returns the reopened database and
/// what was done. The caller has closed its handle on the damaged file (it never had one:
/// recovery mode runs on an in-memory database).
pub fn resolve(
    fs: &dyn Fs,
    paths: &AppPaths,
    clock: &dyn Clock,
    choice: &DatabaseChoice,
    repaired: Option<&Path>,
) -> Result<(Db, RecoveredHow), Error> {
    let files = LibraryFiles::from_paths(paths);
    let now = clock.now();
    let how = match choice {
        DatabaseChoice::RestoreBackup(p) => {
            // Only files in the backups folder can be restored from here.
            ensure!(
                p.parent() == Some(paths.backups_dir.as_path()),
                FsReadDenied,
                detail = "not a backup of this library"
            );
            backup::restore(&files, p, now)?;
            RecoveredHow::Restored
        }
        DatabaseChoice::UseRepaired => {
            let r =
                repaired.ok_or_else(|| err!(DbCorrupt, detail = "nothing could be repaired"))?;
            integrity::swap_in(&files, r, now)?;
            RecoveredHow::UsedRepaired
        }
        DatabaseChoice::StartEmpty => {
            backup::move_aside(&files, "corrupt", now)?;
            RecoveredHow::StartedEmpty
        }
    };
    if let Some(r) = repaired.filter(|r| fs.exists(r)) {
        let _ = fs.remove_file(r);
    }
    let mut db = Db::open_rw(&paths.db_file)?;
    match db.migrate(&MigrateOptions {
        backups_dir: Some(paths.backups_dir.clone()),
        now,
    })? {
        MigrationOutcome::Ready | MigrationOutcome::Migrated { .. } => {}
        other => bail!(DbMigrationFailed, detail = format!("{other:?}")),
    }
    // Statistics are derived data: rebuild them from the sessions that survived.
    if let Err(e) = crate::stats::rebuild::check_and_repair(&mut db) {
        tracing::warn!("statistics rebuild after recovery failed: {e}");
    }
    Ok((db, how))
}

impl AppService {
    /// `ResolveDatabase`: carry out the choice, reopen the library, leave recovery mode.
    pub(super) fn resolve_database(&mut self, choice: &DatabaseChoice) -> Result<(), Error> {
        let reason = self
            .hub
            .diagnostics
            .load()
            .recovery
            .as_ref()
            .map(|r| matches!(r.reason, RecoveryReason::Damaged { .. }));
        if reason != Some(true) {
            return Ok(());
        }
        let (db, how) = resolve(
            &*self.deps.fs,
            &self.deps.paths,
            &*self.deps.clock,
            choice,
            self.repaired_db.as_deref(),
        )?;
        self.repaired_db = None;
        *self
            .flow
            .stats_db
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        self.lib = Library::open(db, lib_deps(&self.deps, &self.check))?;
        self.update_diagnostics(|d| d.recovery = None);
        if let Err(e) = self.lib.sweep_orphans() {
            tracing::warn!(error = %e, "orphan sweep after recovery failed");
        }
        self.refresh_art_index();
        self.publish_library();
        self.notice(Notice::LibraryRecovered {
            how,
            history_rows_lost: None,
        });
        Ok(())
    }
}
