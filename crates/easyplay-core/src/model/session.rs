//! Sessions: one sandbox lifetime each (DATA_MODEL §5, PROCESS_LIFECYCLE §5, ADR-0013).

use serde::{Deserialize, Serialize};

use super::{NetworkAccess, Preset, RuntimeIsolationMode, SlrVariant};
use crate::clock::Timestamp;
use crate::error::ErrorCode;
use crate::ids::{ExecutableId, GameId, RuntimeName, SessionId};

/// A recorded session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Identifier.
    pub id: SessionId,
    /// The game; `None` after the game was removed.
    pub game_id: Option<GameId>,
    /// Game name snapshot at start.
    pub game_name: String,
    /// The executable started.
    pub executable_id: Option<ExecutableId>,
    /// Kind.
    pub kind: SessionKind,
    /// State.
    pub state: SessionState,
    /// Start (wall clock).
    pub started_at: Timestamp,
    /// End (wall clock).
    pub ended_at: Option<Timestamp>,
    /// Monotonic duration; set at end.
    pub duration_ms: Option<u64>,
    /// Monotonic elapsed time at the last heartbeat.
    pub active_ms: u64,
    /// Last heartbeat (wall clock).
    pub last_heartbeat_at: Option<Timestamp>,
    /// Why it ended.
    pub end_reason: Option<EndReason>,
    /// Raw exit status of the sandbox.
    pub exit_status: Option<i32>,
    /// Whether it counts towards statistics.
    pub counted: bool,
    /// The supervisor process.
    pub supervisor: Option<SupervisorIdent>,
    /// PID of `bwrap` (host namespace).
    pub sandbox_pid: Option<u32>,
    /// PID namespace identifier of the sandbox.
    pub pid_ns: Option<u64>,
    /// Settings in effect.
    pub context: SessionContext,
    /// Local UTC offset at start, in minutes.
    pub tz_offset_min: i16,
}

impl Session {
    /// Whether the session is still open.
    pub fn is_open(&self) -> bool {
        self.state != SessionState::Ended
    }
}

/// What a session runs (SECURITY_MODEL §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// The game.
    Game,
    /// An installer.
    Installer,
    /// A tool in the game's prefix.
    Tool,
    /// Trusted prefix preparation.
    Prepare,
    /// Trusted runtime download.
    Provision,
}

impl SessionKind {
    /// Text form for `sessions.kind`.
    pub const fn as_str(self) -> &'static str {
        match self {
            SessionKind::Game => "game",
            SessionKind::Installer => "installer",
            SessionKind::Tool => "tool",
            SessionKind::Prepare => "prepare",
            SessionKind::Provision => "provision",
        }
    }

    /// Parse [`SessionKind::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        [
            SessionKind::Game,
            SessionKind::Installer,
            SessionKind::Tool,
            SessionKind::Prepare,
            SessionKind::Provision,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }

    /// Whether untrusted code runs (taints the prefix).
    pub const fn is_untrusted(self) -> bool {
        matches!(
            self,
            SessionKind::Game | SessionKind::Installer | SessionKind::Tool
        )
    }

    /// Whether the session may count towards playtime.
    pub const fn may_count(self) -> bool {
        matches!(self, SessionKind::Game)
    }
}

/// Session state (PROCESS_LIFECYCLE §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Supervisor exists; sandbox being set up or umu preparing.
    Starting,
    /// A game process has been observed.
    Running,
    /// `umu-run` exited but other processes remain.
    Lingering,
    /// A stop was requested.
    Stopping,
    /// Closed with an end reason.
    Ended,
}

impl SessionState {
    /// Text form for `sessions.state`.
    pub const fn as_str(self) -> &'static str {
        match self {
            SessionState::Starting => "starting",
            SessionState::Running => "running",
            SessionState::Lingering => "lingering",
            SessionState::Stopping => "stopping",
            SessionState::Ended => "ended",
        }
    }

    /// Parse [`SessionState::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        [
            SessionState::Starting,
            SessionState::Running,
            SessionState::Lingering,
            SessionState::Stopping,
            SessionState::Ended,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }

    /// Whether `self → next` is a legal transition of PROCESS_LIFECYCLE §5.
    pub fn can_become(self, next: SessionState) -> bool {
        use SessionState::*;
        matches!(
            (self, next),
            (Starting, Running | Stopping | Ended)
                | (Running, Lingering | Stopping | Ended)
                | (Lingering, Running | Stopping | Ended)
                | (Stopping, Ended)
        )
    }
}

/// Why a session ended (PROCESS_LIFECYCLE §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    /// The main program exited with a code.
    Exited {
        /// Exit code.
        code: i32,
    },
    /// The main program died from a signal.
    Crashed {
        /// Signal number.
        signal: i32,
    },
    /// The user stopped it.
    StoppedByUser,
    /// Forced after the grace period.
    Killed,
    /// The system shut down.
    SystemShutdown,
    /// The launcher (e.g. a store launcher) terminated the session.
    LauncherTerminated,
    /// The supervisor died; closed by recovery from the last heartbeat.
    Interrupted,
    /// The sandbox or supervisor failed to start.
    FailedToStart {
        /// The classified error.
        code: ErrorCode,
    },
}

impl EndReason {
    /// Text form for `sessions.end_reason`.
    pub const fn as_str(self) -> &'static str {
        match self {
            EndReason::Exited { .. } => "exited",
            EndReason::Crashed { .. } => "crashed",
            EndReason::StoppedByUser => "stopped_by_user",
            EndReason::Killed => "killed",
            EndReason::SystemShutdown => "system_shutdown",
            EndReason::LauncherTerminated => "launcher_terminated",
            EndReason::Interrupted => "interrupted",
            EndReason::FailedToStart { .. } => "failed_to_start",
        }
    }

    /// Text for `sessions.end_detail`: exit code, signal number or error code.
    pub fn detail(self) -> Option<String> {
        match self {
            EndReason::Exited { code } => Some(code.to_string()),
            EndReason::Crashed { signal } => Some(signal.to_string()),
            EndReason::FailedToStart { code } => Some(code.as_str().to_owned()),
            _ => None,
        }
    }

    /// Rebuild from the `end_reason` and `end_detail` columns.
    pub fn from_columns(reason: &str, detail: Option<&str>) -> Option<Self> {
        let num = || detail.and_then(|d| d.parse::<i32>().ok());
        Some(match reason {
            "exited" => EndReason::Exited { code: num()? },
            "crashed" => EndReason::Crashed { signal: num()? },
            "stopped_by_user" => EndReason::StoppedByUser,
            "killed" => EndReason::Killed,
            "system_shutdown" => EndReason::SystemShutdown,
            "launcher_terminated" => EndReason::LauncherTerminated,
            "interrupted" => EndReason::Interrupted,
            "failed_to_start" => EndReason::FailedToStart {
                code: detail?.parse().ok()?,
            },
            _ => return None,
        })
    }
}

/// Identity of a supervisor process, immune to PID reuse.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SupervisorIdent {
    /// PID.
    pub pid: u32,
    /// Start time in clock ticks since boot (`/proc/<pid>/stat` field 22).
    pub start_ticks: u64,
    /// `/proc/sys/kernel/random/boot_id`.
    pub boot_id: String,
}

/// The settings a session ran with (shown in history and statistics).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionContext {
    /// Preset.
    pub preset: Option<Preset>,
    /// Network access.
    pub network: Option<NetworkAccess>,
    /// Proton build.
    pub runtime_name: Option<RuntimeName>,
    /// SLR variant.
    pub slr_variant: Option<SlrVariant>,
    /// Isolation mode.
    pub isolation: Option<RuntimeIsolationMode>,
}
