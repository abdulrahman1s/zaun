//! Stopping (PROC-006, PROCESS_LIFECYCLE §6, LOGIC_SPECS §6).
//!
//! The requester side ([`request_stop`]) writes `stop-request` (`graceful` or `force`) into
//! the session runtime directory and signals the supervisor by [`ProcIdentity`]: `SIGTERM`
//! graceful, `SIGUSR1` force. The supervisor side ([`Stopper`]) is a pure state machine of
//! the stages Graceful 1 → Graceful 2 → Force whose [`StopAction`]s the supervisor executes.
//! A `SIGTERM` without a request file is a session or system shutdown and uses a shortened
//! grace period.

use std::path::Path;
use std::time::Duration;

use crate::clock::MonoInstant;
use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::model::Session;
use crate::paths::AppPaths;
use crate::procfs::{ProcIdentity, ProcSource};

use super::classify::StopOrigin;
use super::sys::{self, Sig, SignalError};

/// Graceful or forced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StopMode {
    /// Ask the game to close, force after the grace period.
    Graceful,
    /// Kill the sandbox now.
    Force,
}

impl StopMode {
    /// Content of the `stop-request` file.
    pub const fn as_str(self) -> &'static str {
        match self {
            StopMode::Graceful => "graceful",
            StopMode::Force => "force",
        }
    }
}

/// Grace period of a stop the supervisor did not get a request file for (logout, shutdown).
pub const SYSTEM_STOP_GRACE: Duration = Duration::from_secs(3);
/// Duration of the second graceful stage.
pub const GRACEFUL2: Duration = Duration::from_secs(2);
/// How long the force stage waits for the sandbox to disappear.
pub const FORCE_WAIT: Duration = Duration::from_secs(2);

/// What a stop request achieved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopOutcome {
    /// The supervisor was signalled; it ends the session.
    Signalled,
    /// The supervisor is gone: the caller runs recovery for the row.
    SupervisorGone,
}

/// The supervisor's identity recorded in a row.
pub fn supervisor_identity(s: &Session) -> Option<ProcIdentity> {
    s.supervisor.as_ref().map(|v| ProcIdentity {
        pid: v.pid,
        start_time_ticks: v.start_ticks,
    })
}

/// Ask the supervisor of `session` to stop (LOGIC_SPECS §6). Idempotent.
pub fn request_stop(
    paths: &AppPaths,
    fs: &dyn Fs,
    proc: &dyn ProcSource,
    session: &Session,
    mode: StopMode,
) -> Result<StopOutcome, Error> {
    let Some(id) = supervisor_identity(session) else {
        return Ok(StopOutcome::SupervisorGone);
    };
    if !proc.is_alive(id).unwrap_or(false) {
        return Ok(StopOutcome::SupervisorGone);
    }
    let dir = paths.run_session_dir(session.id);
    fs.create_dir_all(&dir, 0o700)?;
    fs.atomic_write(
        &paths.stop_request_file(session.id),
        format!("{}\n", mode.as_str()).as_bytes(),
        0o600,
    )?;
    let sig = match mode {
        StopMode::Graceful => Sig::Term,
        StopMode::Force => Sig::Usr1,
    };
    match sys::signal_identity(proc, id, sig) {
        Ok(()) => Ok(StopOutcome::Signalled),
        Err(SignalError::Gone) => Ok(StopOutcome::SupervisorGone),
        Err(SignalError::Denied) => Err(Error::new(ErrorCode::SesSignalDenied)),
        Err(SignalError::Other(e)) => Err(Error::new(ErrorCode::SesSignalDenied).with_source(e)),
    }
}

/// Read (and remove) the `stop-request` file: `Some(mode)` when a user request preceded the
/// signal.
pub fn take_stop_request(fs: &dyn Fs, path: &Path) -> Option<StopMode> {
    let bytes = fs.read(path, 64).ok()?;
    let _ = fs.remove_file(path);
    match String::from_utf8_lossy(&bytes).trim() {
        "force" => Some(StopMode::Force),
        _ => Some(StopMode::Graceful),
    }
}

/// A step the supervisor carries out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopAction {
    /// Set the row to `Stopping`.
    MarkStopping,
    /// `SIGTERM` the init shim (which relays it to every process).
    TermInit,
    /// `SIGTERM` the init shim again and every host-visible process of the sandbox.
    TermAll,
    /// `SIGKILL` the sandbox's PID 1 and every member, then the bwrap monitor.
    KillAll,
    /// The sandbox did not disappear after the force stage (`ZN-SES-020`): stop waiting.
    GiveUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Graceful1 { until: MonoInstant },
    Graceful2 { until: MonoInstant },
    Force { until: MonoInstant },
    GaveUp,
}

/// The stop stages of one session.
#[derive(Clone, Copy, Debug)]
pub struct Stopper {
    stage: Stage,
    origin: Option<StopOrigin>,
    forced: bool,
    grace: Duration,
}

impl Stopper {
    /// A stopper with the game's grace period.
    pub fn new(grace: Duration) -> Self {
        Self {
            stage: Stage::Idle,
            origin: None,
            forced: false,
            grace,
        }
    }

    /// Who asked first, if anyone.
    pub fn origin(&self) -> Option<StopOrigin> {
        self.origin
    }

    /// Whether the force stage ran or was requested.
    pub fn forced(&self) -> bool {
        self.forced
    }

    /// Whether a stop is in progress.
    pub fn active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// The next deadline, if a stage is waiting.
    pub fn deadline(&self) -> Option<MonoInstant> {
        match self.stage {
            Stage::Graceful1 { until } | Stage::Graceful2 { until } | Stage::Force { until } => {
                Some(until)
            }
            Stage::Idle | Stage::GaveUp => None,
        }
    }

    /// A request arrives. A second graceful request during a stop is ignored; force always
    /// skips to the last stage.
    pub fn request(
        &mut self,
        mode: StopMode,
        origin: StopOrigin,
        now: MonoInstant,
    ) -> Vec<StopAction> {
        if self.origin.is_none() {
            self.origin = Some(origin);
        }
        let mut out = Vec::new();
        if self.stage == Stage::Idle {
            out.push(StopAction::MarkStopping);
        }
        match (mode, self.stage) {
            (StopMode::Graceful, Stage::Idle) => {
                let grace = if origin == StopOrigin::System {
                    SYSTEM_STOP_GRACE.min(self.grace)
                } else {
                    self.grace
                };
                self.stage = Stage::Graceful1 {
                    until: now.saturating_add(grace),
                };
                out.push(StopAction::TermInit);
            }
            (StopMode::Graceful, _) => {}
            (StopMode::Force, Stage::Force { .. } | Stage::GaveUp) => {}
            (StopMode::Force, _) => out.extend(self.force(now)),
        }
        out
    }

    fn force(&mut self, now: MonoInstant) -> Vec<StopAction> {
        self.forced = true;
        self.stage = Stage::Force {
            until: now.saturating_add(FORCE_WAIT),
        };
        vec![StopAction::KillAll]
    }

    /// Time passes: escalate when a stage's deadline is reached.
    pub fn tick(&mut self, now: MonoInstant) -> Vec<StopAction> {
        match self.stage {
            Stage::Graceful1 { until } if now >= until => {
                self.stage = Stage::Graceful2 {
                    until: now.saturating_add(GRACEFUL2),
                };
                vec![StopAction::TermAll]
            }
            Stage::Graceful2 { until } if now >= until => self.force(now),
            Stage::Force { until } if now >= until => {
                self.stage = Stage::GaveUp;
                vec![StopAction::GiveUp]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // Test fixtures on a temporary directory.
mod tests {
    use super::*;

    fn at(s: u64) -> MonoInstant {
        MonoInstant::from_duration(Duration::from_secs(s))
    }

    #[test]
    fn graceful_escalates_to_force() {
        let mut s = Stopper::new(Duration::from_secs(10));
        assert_eq!(
            s.request(StopMode::Graceful, StopOrigin::User, at(0)),
            vec![StopAction::MarkStopping, StopAction::TermInit]
        );
        assert_eq!(s.deadline(), Some(at(10)));
        assert!(
            s.request(StopMode::Graceful, StopOrigin::User, at(1))
                .is_empty()
        );
        assert!(s.tick(at(9)).is_empty());
        assert_eq!(s.tick(at(10)), vec![StopAction::TermAll]);
        assert_eq!(s.tick(at(12)), vec![StopAction::KillAll]);
        assert!(s.forced());
        assert_eq!(s.tick(at(14)), vec![StopAction::GiveUp]);
        assert_eq!(s.deadline(), None);
        assert_eq!(s.origin(), Some(StopOrigin::User));
        // A stop that ignores SIGTERM ends within grace + 4 s.
    }

    #[test]
    fn force_skips_and_system_grace_is_short() {
        let mut s = Stopper::new(Duration::from_secs(30));
        assert_eq!(
            s.request(StopMode::Force, StopOrigin::User, at(0)),
            vec![StopAction::MarkStopping, StopAction::KillAll]
        );
        assert!(
            s.request(StopMode::Force, StopOrigin::User, at(0))
                .is_empty()
        );
        let mut s = Stopper::new(Duration::from_secs(30));
        s.request(StopMode::Graceful, StopOrigin::System, at(0));
        assert_eq!(s.deadline(), Some(at(3)));
        assert_eq!(
            s.request(StopMode::Force, StopOrigin::User, at(1)),
            vec![StopAction::KillAll]
        );
        assert_eq!(s.origin(), Some(StopOrigin::System));
    }

    #[test]
    fn stop_request_file() {
        let dir = tempfile::tempdir().unwrap();
        let fs = crate::fsx::RealFs::new();
        let p = dir.path().join("stop-request");
        assert_eq!(take_stop_request(&fs, &p), None);
        std::fs::write(&p, "force\n").unwrap();
        assert_eq!(take_stop_request(&fs, &p), Some(StopMode::Force));
        assert!(!p.exists());
        std::fs::write(&p, "graceful\n").unwrap();
        assert_eq!(take_stop_request(&fs, &p), Some(StopMode::Graceful));
    }
}
