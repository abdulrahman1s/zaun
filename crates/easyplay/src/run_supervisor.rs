//! Read the plan file, construct supervisor dependencies, call `session::supervisor::run`.
//! Must not initialise eframe, fonts, GPU or the instance socket (PROC-003).
//!
//! Order (PROCESS_LIFECYCLE §3, §9): read the plan; open the library (it must exist with
//! the current schema: the launcher created it); run recovery, keeping this session's
//! directories; install the signal handlers; supervise. The process exit code is the
//! supervisor's (0 ended, 1 not started, 3 prefix lock held, 4 sandbox not spawned).

use std::path::Path;
use std::sync::Arc;

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::launch::plan_file;
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::procfs::{ProcSource, RealProc};
use easyplay_core::session::{
    self, RealSpawner, RecoveryOptions, StopTrigger, SupervisorConfig, SupervisorDeps,
    SupervisorOutcome,
};
use easyplay_core::storage::Db;

/// Supervise the session described by `plan`; returns the process exit code.
pub fn run(plan_path: &Path) -> Result<u8, Error> {
    let env = PathEnv::from_process();
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("detail", e))?;
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let proc: Arc<dyn ProcSource> = Arc::new(RealProc::new(Arc::clone(&fs)));
    let plan = plan_file::read(&*fs, plan_path)?;
    tracing::info!(session = %plan.session.id, "supervisor starting");

    let mut db = Db::open_rw(&paths.db_file)?;
    let opts = RecoveryOptions {
        keep: vec![plan.session.id],
        ..RecoveryOptions::default()
    };
    if let Err(e) = session::recover(&mut db, &*proc, &*fs, &paths, &*clock, &opts) {
        tracing::warn!("recovery at supervisor start failed: {e}");
    }

    let trigger =
        StopTrigger::new().map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?;
    trigger
        .install_signal_handlers()
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?;
    let mut cfg = SupervisorConfig::new(env.uid());
    cfg.runtime_store = Some(paths.runtimes_dir.clone());
    let deps = SupervisorDeps {
        store: Box::new(db),
        clock,
        proc,
        fs,
        spawner: Box::new(RealSpawner),
        trigger,
    };
    let outcome = session::supervisor::run(&plan, &cfg, deps);
    match &outcome {
        SupervisorOutcome::NotStarted(e) => tracing::error!("supervisor not started: {e}"),
        SupervisorOutcome::LockHeld => tracing::warn!("the game is already running"),
        SupervisorOutcome::Ended {
            reason, counted, ..
        } => tracing::info!(reason = reason.as_str(), counted, "supervisor done"),
    }
    Ok(outcome.exit_code())
}
