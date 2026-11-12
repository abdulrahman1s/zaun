//! The launch orchestrator (LAUNCH-004, LOGIC_SPECS §3): drives one launch from validation to
//! the hand-off to a supervisor, reporting each [`LaunchState`].
//!
//! M6 scope: Validating → ResolvingRuntime → Planning → Spawning → Starting. When the runtime
//! needs a Provision or Prepare session, or a Shared-mode audit, the launch stops with
//! [`LaunchError::NeedsProvision`] (the sub-sessions arrive in M7). Prompts raised by
//! preflight (`ExeChanged`, `GrantTargetChanged`) are returned as [`LaunchError::Prompt`];
//! a headless caller without a terminal fails with the prompt's description
//! (CONFIGURATION §6), the GUI flow re-enters after the answer.
//!
//! The orchestrator never waits for the game: from `Starting` on, the session belongs to
//! the supervisor and is observed through its row.

use std::sync::Arc;

use crate::clock::Clock;
use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::host::HostFacts;
use crate::ids::{ExecutableId, LaunchId, SessionId};
use crate::model::{Game, SessionKind};
use crate::runtime::{ProvisionNeed, Resolution, RuntimeService};
use crate::sandbox::ValidationInputs;
use crate::session::{SessionHandle, SessionLauncher};

use super::planner::{
    HostEnv, LaunchTarget, PlanRequest, PlannedLaunch, plan_launch, with_context,
};
use super::preflight::{
    FingerprintCheck, GrantChange, PrefixLocks, PreflightInput, PreflightReport, STAT_DEADLINE,
    preflight,
};
use super::state::{Launch, LaunchState};

/// What to launch.
#[derive(Clone, Copy, Debug)]
pub struct LaunchRequest<'a> {
    /// The game.
    pub game: &'a Game,
    /// Which executable (default: the primary).
    pub exe: Option<ExecutableId>,
    /// Extra arguments for this run.
    pub extra_args: &'a [String],
    /// The new session's ID.
    pub session_id: SessionId,
}

/// What the orchestrator uses.
pub struct OrchestratorDeps<'a> {
    /// Host facts.
    pub host: &'a HostFacts,
    /// Process-wide planning inputs.
    pub env: &'a HostEnv<'a>,
    /// Runtime resolution.
    pub runtimes: &'a dyn RuntimeService,
    /// Starts the supervisor.
    pub launcher: &'a dyn SessionLauncher,
    /// Prefix lock probe (`session::FlockLocks`).
    pub locks: &'a dyn PrefixLocks,
    /// Filesystem for preflight.
    pub fs: Arc<dyn Fs>,
    /// Time (state timeline).
    pub clock: &'a dyn Clock,
}

/// A question preflight raised (LOGIC_SPECS §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchPrompt {
    /// The executable changed since it was added.
    ExeChanged {
        /// Current size.
        size: u64,
        /// Current modification time.
        mtime_ns: i64,
    },
    /// A granted folder now resolves elsewhere.
    GrantTargetChanged(Vec<GrantChange>),
}

impl LaunchPrompt {
    /// The description a non-interactive caller fails with.
    pub fn description(&self) -> String {
        match self {
            LaunchPrompt::ExeChanged { .. } => {
                "the program file changed since it was added; confirm it in EasyPlay".into()
            }
            LaunchPrompt::GrantTargetChanged(c) => format!(
                "{} granted folder(s) now point somewhere else; confirm them in EasyPlay",
                c.len()
            ),
        }
    }
}

/// Why a launch did not reach `Starting`.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    /// A check, the planner or the launcher failed.
    #[error(transparent)]
    Failed(#[from] Error),
    /// The user must answer a prompt first.
    #[error("{}", .0.description())]
    Prompt(LaunchPrompt),
    /// A Provision or Prepare session must run first (M7).
    #[error("the compatibility layer must be installed first ({})", .0.protonpath())]
    NeedsProvision(ProvisionNeed),
}

impl From<LaunchError> for Error {
    fn from(e: LaunchError) -> Self {
        match e {
            LaunchError::Failed(e) => e,
            LaunchError::Prompt(p) => {
                let code = match &p {
                    LaunchPrompt::ExeChanged { .. } => ErrorCode::LibInvalidField,
                    LaunchPrompt::GrantTargetChanged(_) => ErrorCode::SbxGrantTargetChanged,
                };
                Error::new(code).with("detail", p.description())
            }
            LaunchError::NeedsProvision(n) => Error::new(ErrorCode::RtNotInstalledOffline)
                .with("detail", format!("needs {}", n.protonpath())),
        }
    }
}

/// A launch handed to its supervisor.
#[derive(Debug)]
pub struct LaunchOutcome {
    /// The session.
    pub handle: SessionHandle,
    /// Preflight warnings (low disk space, non-blocking policy issues).
    pub report: PreflightReport,
    /// The plan that was started.
    pub planned: PlannedLaunch,
    /// The state timeline.
    pub launch: Launch,
}

fn advance(
    launch: &mut Launch,
    next: LaunchState,
    clock: &dyn Clock,
    on_state: &mut dyn FnMut(&LaunchState),
) {
    if launch.advance(next, clock.mono()).is_ok() {
        on_state(launch.state());
    }
}

/// Run one launch to the hand-off. `on_state` sees every state entered.
pub fn launch(
    req: &LaunchRequest<'_>,
    deps: &OrchestratorDeps<'_>,
    on_state: &mut dyn FnMut(&LaunchState),
) -> Result<LaunchOutcome, LaunchError> {
    let clock = deps.clock;
    let mut state = Launch::start(LaunchId::generate(clock), clock.mono());
    on_state(state.state());
    let game = req.game;
    let exe = match req.exe {
        Some(id) => game.executables.iter().find(|e| e.id == id),
        None => game.primary(),
    }
    .ok_or_else(|| {
        Error::new(ErrorCode::LibInvalidField).with("detail", "the game has no such executable")
    })?;

    // Resolution is pure (catalogue snapshot); preflight needs its result for the context.
    let resolution = deps.runtimes.resolve(&game.runtime, Some(game));
    let runtime = match &resolution {
        Resolution::Ready(rt) => Some(rt.clone()),
        _ => None,
    };
    let target = LaunchTarget::Game {
        exe,
        extra_args: req.extra_args,
    };
    let plan_req = PlanRequest {
        session_id: req.session_id,
        target,
        game: Some(game),
        runtime: runtime.as_ref(),
        proton_codename: None,
        network_override: None,
        host: deps.host,
        env: deps.env,
    };

    // Validating.
    let report = match &runtime {
        Some(rt) => {
            let prefix_lock = deps.env.paths.prefix_lock(game.id);
            let checked = with_context(&plan_req, |ctx, _, _| {
                preflight(&PreflightInput {
                    game,
                    exe,
                    kind: SessionKind::Game,
                    host: deps.host,
                    validation: ValidationInputs::from_context(ctx, deps.host),
                    fs: Arc::clone(&deps.fs),
                    mounts: deps.env.mounts,
                    init_binary: deps.env.init_binary,
                    prefix_lock: &prefix_lock,
                    locks: deps.locks,
                    data_dir: &deps.env.paths.data_dir,
                    isolation: Some((rt.isolation, deps.host.fs.reflink_ok)),
                    deadline: STAT_DEADLINE,
                })
            })
            .map_err(Error::from)?;
            checked.map_err(Error::from)?
        }
        None => {
            // Without a runtime the context cannot be built; the lock is still checked.
            if deps.locks.is_held(&deps.env.paths.prefix_lock(game.id)) {
                return Err(Error::new(ErrorCode::SesAlreadyRunning).into());
            }
            PreflightReport {
                fingerprint: FingerprintCheck::Unchanged,
                grant_changes: Vec::new(),
                warnings: Vec::new(),
                low_space: None,
            }
        }
    };
    if let FingerprintCheck::Changed { size, mtime_ns } = report.fingerprint {
        return Err(LaunchError::Prompt(LaunchPrompt::ExeChanged {
            size,
            mtime_ns,
        }));
    }
    if !report.grant_changes.is_empty() {
        return Err(LaunchError::Prompt(LaunchPrompt::GrantTargetChanged(
            report.grant_changes.clone(),
        )));
    }

    // ResolvingRuntime.
    advance(&mut state, LaunchState::ResolvingRuntime, clock, on_state);
    match resolution {
        Resolution::Ready(_) => {}
        Resolution::NeedsProvision(need) => return Err(LaunchError::NeedsProvision(need)),
        Resolution::Unavailable(r) => {
            return Err(Error::new(r.code()).with("reason", format!("{r:?}")).into());
        }
    }

    // Planning.
    advance(&mut state, LaunchState::Planning, clock, on_state);
    let planned = plan_launch(&plan_req).map_err(Error::from)?;

    // Spawning.
    advance(&mut state, LaunchState::Spawning, clock, on_state);
    let handle = deps.launcher.start(&planned.plan)?;
    advance(
        &mut state,
        LaunchState::Starting { phase: None },
        clock,
        on_state,
    );
    Ok(LaunchOutcome {
        handle,
        report,
        planned,
        launch: state,
    })
}
