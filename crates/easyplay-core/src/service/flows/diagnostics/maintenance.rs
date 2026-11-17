//! The start-up maintenance job (SVC-010, PERSISTENCE §7, §10): once per start at
//! `Background` priority, on its own read-write connection. Integrity check, then the daily
//! backup (only after a passing check), the statistics consistency check (rebuilding the
//! aggregates on a mismatch), pruning of `session_events` older than 90 days, and the
//! artwork sweep (unreferenced originals and thumbnails, the 512 MiB thumbnail cap).
//!
//! Session recovery and the retry of interrupted removals run earlier, before the first
//! snapshot (they change what the library shows).

use std::path::PathBuf;
use std::time::Duration;

use crate::clock::Timestamp;
use crate::fsx::Fs;
use crate::jobs::{JobLabel, JobPriority};
use crate::paths::AppPaths;
use crate::service::prelude::*;
use crate::storage::{ArtworkRepo, Db, SessionRepo};

/// `session_events` are kept this long.
pub const EVENTS_RETENTION: Duration = Duration::from_secs(90 * 24 * 3600);

/// What the job did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    /// `quick_check` problems (empty: the database is sound).
    pub integrity_problems: Vec<String>,
    /// The daily backup taken.
    pub backup: Option<PathBuf>,
    /// The statistics aggregates were rebuilt.
    pub stats_rebuilt: bool,
    /// `session_events` rows deleted.
    pub events_pruned: u64,
    /// Artwork files removed.
    pub artwork_removed: usize,
    /// Steps that failed (logged).
    pub failures: Vec<String>,
}

/// Run every step on `db` (any thread).
pub fn run(db: &mut Db, fs: &dyn Fs, paths: &AppPaths, now: Timestamp) -> MaintenanceReport {
    let mut r = MaintenanceReport::default();
    let failed = |what: &str, e: &dyn std::fmt::Display, r: &mut MaintenanceReport| {
        tracing::warn!(error = %e, "maintenance: {what} failed");
        r.failures.push(what.to_owned());
    };
    match crate::storage::integrity::quick_check(db) {
        Ok(crate::storage::integrity::IntegrityReport::Ok) => {
            match crate::storage::backup::run_daily(db, &paths.backups_dir, now) {
                Ok(b) => r.backup = b,
                Err(e) => failed("daily backup", &e, &mut r),
            }
        }
        Ok(crate::storage::integrity::IntegrityReport::Problems(p)) => {
            tracing::error!(
                problems = p.len(),
                "maintenance: integrity check found problems"
            );
            r.integrity_problems = p;
        }
        Err(e) => failed("integrity check", &e, &mut r),
    }
    match crate::stats::rebuild::check_and_repair(db) {
        Ok(rebuilt) => r.stats_rebuilt = rebuilt,
        Err(e) => failed("statistics check", &e, &mut r),
    }
    let cutoff = Timestamp::from_millis(
        now.as_millis()
            .saturating_sub(i64::try_from(EVENTS_RETENTION.as_millis()).unwrap_or(i64::MAX)),
    );
    match db.prune_session_events(cutoff) {
        Ok(n) => r.events_pruned = n,
        Err(e) => failed("session event pruning", &e, &mut r),
    }
    match db.referenced_artwork_hashes() {
        Ok(refs) => {
            let s = crate::artwork::store::sweep(
                fs,
                paths,
                &refs,
                crate::artwork::store::SweepOptions {
                    now_ns: now.as_millis().saturating_mul(1_000_000),
                    thumb_cap: crate::artwork::store::THUMB_CACHE_CAP,
                },
            );
            r.artwork_removed = s.originals_removed + s.thumbs_removed;
        }
        Err(e) => failed("artwork sweep", &e, &mut r),
    }
    r
}

impl AppService {
    /// Queue the maintenance job (after the start-up sequence).
    pub(crate) fn start_maintenance(&self) {
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        let now = self.deps.clock.now();
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("maintenance"),
            move |_| {
                let report = match Db::open_rw(&paths.db_file) {
                    Ok(mut db) => run(&mut db, &*fs, &paths, now),
                    Err(e) => {
                        tracing::warn!(error = %e, "maintenance: database not opened");
                        MaintenanceReport {
                            failures: vec!["open".to_owned()],
                            ..MaintenanceReport::default()
                        }
                    }
                };
                post(Internal::Flow(Box::new(FlowDone::Maintenance(report))));
            },
        );
    }

    pub(crate) fn on_maintenance(&mut self, r: MaintenanceReport) -> Result<()> {
        tracing::info!(
            backup = r.backup.is_some(),
            stats_rebuilt = r.stats_rebuilt,
            events_pruned = r.events_pruned,
            artwork_removed = r.artwork_removed,
            failures = r.failures.len(),
            "maintenance finished"
        );
        if r.stats_rebuilt {
            match self.lib.reload() {
                Ok(()) => self.publish_library(),
                Err(e) => tracing::warn!(error = %e, "library reload after rebuild failed"),
            }
        }
        ensure!(
            r.integrity_problems.is_empty(),
            DbCorrupt,
            detail = r.integrity_problems.join("; ")
        );
        Ok(())
    }
}
