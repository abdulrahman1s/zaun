//! Starting a supervisor (PROC-008, PROCESS_LIFECYCLE §2).
//!
//! [`RealLauncher::start`]: create the session runtime directory (0700) and write
//! `plan.json` (0600) there; create the session log directory and `plan.txt`; start
//! `<easyplay> supervise --plan <file>` detached (double fork with `setsid`, stdin
//! `/dev/null`, stdout and stderr to `supervisor.log`, environment reduced to `PATH`, `HOME`,
//! `XDG_*`, `LANG`, `EASYPLAY_*`, every other descriptor closed); then wait up to 5 s for the
//! supervisor's row (polling every 50 ms). A supervisor that exits early leaves a
//! `start-failed` file with its error code, which is reported instead of waiting.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::clock::Clock;
use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::ids::SessionId;
use crate::launch::{LaunchPlan, plan_file};
use crate::logging::Redactor;
use crate::model::Session;
use crate::paths::AppPaths;
use crate::procfs::{ProcIdentity, ProcSource};
use crate::storage::{Db, SessionRepo};

use super::stop::{StopMode, StopOutcome, request_stop, supervisor_identity};
use super::supervisor::START_FAILED_FILE;
use super::sys;

/// Time the launcher waits for the supervisor's row.
pub const START_TIMEOUT: Duration = Duration::from_secs(5);
/// Poll interval while waiting.
pub const START_POLL: Duration = Duration::from_millis(50);

/// A started session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionHandle {
    /// The session.
    pub session_id: SessionId,
    /// Its supervisor.
    pub supervisor: ProcIdentity,
}

/// Starts and stops sessions (PROCESS_LIFECYCLE §12). Implemented by [`RealLauncher`] and
/// `easyplay_testkit::FakeLauncher`.
pub trait SessionLauncher: Send + Sync {
    /// Start a supervisor for `plan` and wait until it has inserted the session row.
    fn start(&self, plan: &LaunchPlan) -> Result<SessionHandle, Error>;
    /// Ask the supervisor of `session` to stop.
    fn request_stop(&self, session: &Session, mode: StopMode) -> Result<StopOutcome, Error>;
}

/// Which variables a supervisor inherits.
pub fn supervisor_env_allowed(name: &str) -> bool {
    matches!(name, "PATH" | "HOME" | "LANG")
        || name.starts_with("XDG_")
        || name.starts_with("EASYPLAY_")
}

/// The real launcher.
pub struct RealLauncher {
    /// The `easyplay` binary (`supervise` verb).
    pub exe: PathBuf,
    /// Locations.
    pub paths: AppPaths,
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Process information.
    pub proc: Arc<dyn ProcSource>,
    /// Time (only for the wait loop's bookkeeping).
    pub clock: Arc<dyn Clock>,
    /// The environment to filter for the supervisor.
    pub env: Vec<(OsString, OsString)>,
    /// How long to wait for the row.
    pub start_timeout: Duration,
}

impl RealLauncher {
    /// A launcher for `exe` with the process environment.
    pub fn new(
        exe: PathBuf,
        paths: AppPaths,
        fs: Arc<dyn Fs>,
        proc: Arc<dyn ProcSource>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            exe,
            paths,
            fs,
            proc,
            clock,
            env: std::env::vars_os().collect(),
            start_timeout: START_TIMEOUT,
        }
    }

    fn supervisor_log(&self, plan: &LaunchPlan) -> PathBuf {
        plan.paths.log_dir.join("supervisor.log")
    }

    fn log_tail(&self, plan: &LaunchPlan) -> String {
        let text = self
            .fs
            .read(&self.supervisor_log(plan), 1 << 20)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(10)..].join("\n")
    }

    #[allow(clippy::disallowed_methods)] // session::launcher spawns the supervisor (ARCH-002).
    fn spawn(&self, plan_path: &Path, plan: &LaunchPlan) -> Result<(), Error> {
        use std::os::unix::fs::OpenOptionsExt;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(self.supervisor_log(plan))
            .map_err(|e| Error::new(ErrorCode::SesSupervisorNotStarted).with_source(e))?;
        let log2 = log
            .try_clone()
            .map_err(|e| Error::new(ErrorCode::SesSupervisorNotStarted).with_source(e))?;
        let mut cmd = Command::new(&self.exe);
        cmd.arg("supervise")
            .arg("--plan")
            .arg(plan_path)
            .env_clear()
            .envs(
                self.env
                    .iter()
                    .filter(|(k, _)| k.to_str().is_some_and(supervisor_env_allowed))
                    .map(|(k, v)| (k, v)),
            )
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log2));
        sys::configure_detached(&mut cmd);
        let mut intermediate = cmd.spawn().map_err(|e| {
            Error::new(ErrorCode::SesSupervisorNotStarted)
                .with("path", self.exe.display())
                .with_source(e)
        })?;
        // The intermediate exits at once; reap it.
        let _ = intermediate.wait();
        Ok(())
    }
}

impl SessionLauncher for RealLauncher {
    fn start(&self, plan: &LaunchPlan) -> Result<SessionHandle, Error> {
        let id = plan.session.id;
        let fs = &*self.fs;
        fs.create_dir_all(&plan.paths.runtime_dir, 0o700)?;
        fs.set_mode(&plan.paths.runtime_dir, 0o700)?;
        let plan_path = plan.paths.runtime_dir.join("plan.json");
        plan_file::write(fs, &plan_path, plan)?;
        fs.create_dir_all(&plan.paths.log_dir, 0o700)?;
        let redact = Redactor::new(self.paths.home.as_deref(), None, None);
        let _ = fs.atomic_write(
            &plan.paths.log_dir.join("plan.txt"),
            plan_file::render_text(plan, &redact).as_bytes(),
            0o600,
        );
        self.spawn(&plan_path, plan)?;

        let start = self.clock.mono();
        let failed_file = plan.paths.runtime_dir.join(START_FAILED_FILE);
        let mut db: Option<crate::storage::ReadDb> = None;
        loop {
            if let Ok(bytes) = fs.read(&failed_file, 256) {
                let text = String::from_utf8_lossy(&bytes).trim().to_owned();
                let code = text.parse().unwrap_or(ErrorCode::SesSupervisorNotStarted);
                let _ = fs.remove_file(&failed_file);
                return Err(Error::new(code).with("log", self.log_tail(plan)));
            }
            if db.is_none() {
                db = Db::open_ro(&self.paths.db_file).ok();
            }
            if let Some(d) = db.as_mut()
                && let Ok(Some(s)) = d.session(id)
                && let Some(sup) = supervisor_identity(&s)
            {
                return Ok(SessionHandle {
                    session_id: id,
                    supervisor: sup,
                });
            }
            if self.clock.mono().saturating_duration_since(start) >= self.start_timeout {
                return Err(
                    Error::new(ErrorCode::SesSupervisorNotStarted).with("log", self.log_tail(plan))
                );
            }
            std::thread::sleep(START_POLL);
        }
    }

    fn request_stop(&self, session: &Session, mode: StopMode) -> Result<StopOutcome, Error> {
        request_stop(&self.paths, &*self.fs, &*self.proc, session, mode)
    }
}
