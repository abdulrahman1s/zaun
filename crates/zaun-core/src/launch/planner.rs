//! `plan_launch`: combine the `SandboxPlan`, the `UmuInvocation`, session metadata and paths
//! into the [`LaunchPlan`] for any session kind (LOGIC_SPECS §3 "Planning").
//!
//! Pure like the sandbox planner: reads the host only through `HostFacts` and `Fs` reads,
//! writes nothing, spawns nothing. Host actions are listed in the plan and executed by the
//! supervisor (never for dry runs).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::plan_file::{
    AuditAfter, LAUNCH_PLAN_FORMAT, LaunchPlan, PlanFd, PlanPaths, PlanSandbox, PlanSession,
};
use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, MountTable};
use crate::host::HostFacts;
use crate::ids::SessionId;
use crate::model::{
    EnvVar, Executable, Game, LaunchConfig, NetworkAccess, ResolvedRuntime, RuntimeIsolationMode,
    SandboxPolicy, SessionContext, SessionKind, Tool, UserDir,
};
use crate::paths::AppPaths;
use crate::sandbox::plan::{FD_INIT_STATUS, INIT_PATH};
use crate::sandbox::{
    self, GameContext, GamePaths, LaunchInputs, PolicyIssue, RenderError, RenderedSandbox,
    SandboxContext, SandboxError, SandboxPlan, SessionDirs,
};
use crate::umu::{self, CommandError, ProtonPath, Target, UmuGame, UmuInvocation, UmuOptions};

/// What to run.
#[derive(Clone, Copy, Debug)]
pub enum LaunchTarget<'a> {
    /// A game executable with extra arguments appended after the stored ones.
    Game {
        /// The executable (primary or alternate).
        exe: &'a Executable,
        /// Extra arguments (`zaun launch … -- ARGS`).
        extra_args: &'a [String],
    },
    /// An installer executable.
    Installer {
        /// The installer.
        exe: &'a Executable,
    },
    /// A tool in the game's prefix.
    Tool(&'a Tool),
    /// Create or upgrade the prefix.
    Prepare,
    /// Install a compatibility layer (no game).
    Provision,
}

impl LaunchTarget<'_> {
    /// The session kind.
    pub const fn kind(&self) -> SessionKind {
        match self {
            LaunchTarget::Game { .. } => SessionKind::Game,
            LaunchTarget::Installer { .. } => SessionKind::Installer,
            LaunchTarget::Tool(_) => SessionKind::Tool,
            LaunchTarget::Prepare => SessionKind::Prepare,
            LaunchTarget::Provision => SessionKind::Provision,
        }
    }
}

/// Process-wide inputs (the user, the launcher's environment, preferences).
#[derive(Clone, Copy)]
pub struct HostEnv<'a> {
    /// User ID.
    pub uid: u32,
    /// Primary group ID.
    pub gid: u32,
    /// User name.
    pub user_name: &'a str,
    /// Primary group name.
    pub group_name: &'a str,
    /// Real home.
    pub home: &'a Path,
    /// Zaun's locations.
    pub paths: &'a AppPaths,
    /// The launcher's environment.
    pub host_env: &'a BTreeMap<String, String>,
    /// `meta.install_id`.
    pub install_id: &'a str,
    /// Host path of `zaun-init`.
    pub init_binary: &'a Path,
    /// Developer Mode.
    pub developer_mode: bool,
    /// A seccomp program can be built.
    pub seccomp_available: bool,
    /// Verbose launch logging.
    pub verbose_log: bool,
    /// Global user variables (preferences).
    pub global_env: &'a [EnvVar],
    /// XDG user directories.
    pub user_dirs: &'a [(UserDir, PathBuf)],
    /// Root folders of the other games.
    pub other_game_roots: &'a [PathBuf],
    /// Mount table.
    pub mounts: &'a MountTable,
    /// Filesystem (read-only use).
    pub fs: &'a dyn Fs,
}

/// One planning request.
#[derive(Clone, Copy)]
pub struct PlanRequest<'a> {
    /// The new session's ID.
    pub session_id: SessionId,
    /// What to run.
    pub target: LaunchTarget<'a>,
    /// The game (required for every kind but Provision).
    pub game: Option<&'a Game>,
    /// The resolved runtime (required except for Provision with a codename).
    pub runtime: Option<&'a ResolvedRuntime>,
    /// `PROTONPATH` codename or build name for Provision / network Prepare.
    pub proton_codename: Option<&'a str>,
    /// Network override (Prepare, Provision, tools with consent).
    pub network_override: Option<NetworkAccess>,
    /// Host facts.
    pub host: &'a HostFacts,
    /// Process-wide inputs.
    pub env: &'a HostEnv<'a>,
}

/// The planner's full output: the file contents plus what produced them (inspector,
/// `sandbox print --format json`, explanations).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedLaunch {
    /// The plan file.
    pub plan: LaunchPlan,
    /// The sandbox plan (mounts with origins, environment with origins).
    pub sandbox: SandboxPlan,
    /// The umu invocation.
    pub umu: UmuInvocation,
    /// The rendered sandbox (option groups).
    pub rendered: RenderedSandbox,
}

/// Planning failures.
#[derive(Debug, thiserror::Error)]
pub enum LaunchPlanError {
    /// The request is incomplete (no game for a game kind, no runtime, …).
    #[error("invalid planning request: {0}")]
    Request(&'static str),
    /// bubblewrap or umu was not found by the host probe.
    #[error("host requirement missing: {0}")]
    Host(ErrorCode),
    /// The umu invocation cannot be built.
    #[error(transparent)]
    Umu(#[from] CommandError),
    /// The sandbox planner rejected the launch.
    #[error("sandbox planning failed")]
    Sandbox(Vec<PolicyIssue>),
    /// Rendering failed.
    #[error(transparent)]
    Render(#[from] RenderError),
    /// The sandbox plan and the umu invocation disagree (a bug).
    #[error("plan inconsistency: {0}")]
    Inconsistent(String),
}

impl LaunchPlanError {
    /// The blocking issues, when the sandbox planner failed.
    pub fn issues(&self) -> &[PolicyIssue] {
        match self {
            LaunchPlanError::Sandbox(i) => i,
            _ => &[],
        }
    }
}

impl From<LaunchPlanError> for Error {
    fn from(e: LaunchPlanError) -> Self {
        match e {
            LaunchPlanError::Sandbox(issues) => SandboxError::Plan(issues).into(),
            LaunchPlanError::Render(r) => SandboxError::Render(r).into(),
            LaunchPlanError::Host(code) => Error::new(code),
            LaunchPlanError::Umu(CommandError::InvalidVerb(v)) => {
                Error::new(ErrorCode::LibInvalidField).with("verb", v)
            }
            other => Error::new(ErrorCode::SbxPlanInvariant).with_source(other),
        }
    }
}

/// Owned per-request values the borrowed [`SandboxContext`] points into.
struct Owned {
    session: SessionDirs,
    game_paths: Option<GamePaths>,
    exe: Option<PathBuf>,
    launch_args: Vec<String>,
    default_launch: LaunchConfig,
    provision_policy: SandboxPolicy,
}

fn game_exe<'a>(req: &PlanRequest<'a>) -> Option<&'a Executable> {
    match req.target {
        LaunchTarget::Game { exe, .. } | LaunchTarget::Installer { exe } => Some(exe),
        _ => req.game.and_then(Game::primary),
    }
}

fn proton_path(req: &PlanRequest<'_>) -> Result<ProtonPath, LaunchPlanError> {
    let kind = req.target.kind();
    let network_prepare =
        kind == SessionKind::Prepare && req.network_override == Some(NetworkAccess::Full);
    match (req.proton_codename, req.runtime) {
        (Some(name), _) if kind == SessionKind::Provision || network_prepare => {
            Ok(ProtonPath::Name(name.to_owned()))
        }
        (_, Some(rt)) => Ok(ProtonPath::Path(rt.proton_path_sandbox.clone())),
        _ => Err(LaunchPlanError::Request(
            "no compatibility layer was resolved",
        )),
    }
}

/// Build the umu invocation and the sandbox context of `req` and hand both to `f`. Used by
/// [`plan_launch`] and by callers that need the same context for preflight
/// (`sandbox::ValidationInputs::from_context`).
pub fn with_context<R>(
    req: &PlanRequest<'_>,
    f: impl FnOnce(&SandboxContext<'_>, &UmuInvocation, &SandboxPolicy) -> R,
) -> Result<R, LaunchPlanError> {
    with_context_in(req, None, f)
}

/// [`with_context`] with the prefix at `prefix` instead of the game's own
/// (`AppPaths::prefix_dir`): a program run in a Steam game's prefix (ADR-0031 §6).
fn with_context_in<R>(
    req: &PlanRequest<'_>,
    prefix: Option<&Path>,
    f: impl FnOnce(&SandboxContext<'_>, &UmuInvocation, &SandboxPolicy) -> R,
) -> Result<R, LaunchPlanError> {
    let kind = req.target.kind();
    let Some(umu_facts) = &req.host.umu else {
        return Err(LaunchPlanError::Host(ErrorCode::HostUmuNotFound));
    };
    let game = match (kind, req.game) {
        (SessionKind::Provision, _) => None,
        (_, Some(g)) => Some(g),
        (_, None) => {
            return Err(LaunchPlanError::Request(
                "this kind of session needs a game",
            ));
        }
    };
    let paths = req.env.paths;
    let exe = if game.is_some() {
        match game_exe(req) {
            Some(e) => Some(e.canonical_path.clone()),
            None => return Err(LaunchPlanError::Request("the game has no executable")),
        }
    } else {
        None
    };
    let launch_args = match req.target {
        LaunchTarget::Game { exe, extra_args } => {
            let stored = exe
                .args_override
                .clone()
                .or_else(|| game.map(|g| g.launch.args.clone()))
                .unwrap_or_default();
            stored
                .into_iter()
                .chain(extra_args.iter().cloned())
                .collect()
        }
        LaunchTarget::Installer { exe } => exe.args_override.clone().unwrap_or_default(),
        _ => Vec::new(),
    };
    let owned = Owned {
        session: SessionDirs::new(paths, req.session_id),
        game_paths: game.map(|g| {
            let mut gp = GamePaths::new(paths, g.id);
            if let Some(p) = prefix {
                gp.prefix = p.to_path_buf();
            }
            gp
        }),
        exe,
        launch_args,
        default_launch: LaunchConfig::default(),
        provision_policy: sandbox::offline(),
    };

    let umu_target = match req.target {
        LaunchTarget::Game { .. } => Target::Game {
            exe: owned.exe.as_deref().unwrap_or(Path::new("")),
            args: &owned.launch_args,
        },
        LaunchTarget::Installer { .. } => Target::Installer {
            exe: owned.exe.as_deref().unwrap_or(Path::new("")),
            args: &owned.launch_args,
        },
        LaunchTarget::Tool(t) => Target::Tool(t),
        LaunchTarget::Prepare => Target::Prepare,
        LaunchTarget::Provision => Target::Provision,
    };
    let store = game.and_then(|g| g.store.as_ref()).map(|s| s.as_str());
    let umu_game = owned.game_paths.as_ref().map(|gp| UmuGame {
        prefix: &gp.prefix,
        umu_id: game.and_then(|g| g.umu_id.as_deref()),
        store,
    });
    let invocation = umu::build(
        &umu_target,
        umu_game.as_ref(),
        &proton_path(req)?,
        umu_facts,
        &UmuOptions {
            verbose_log: req.env.verbose_log,
            network_prepare: kind == SessionKind::Prepare
                && req.network_override == Some(NetworkAccess::Full),
        },
    )?;

    let launch = game.map_or(&owned.default_launch, |g| &g.launch);
    let game_ctx = match (game, &owned.game_paths, &owned.exe) {
        (Some(g), Some(gp), Some(exe)) => Some(GameContext {
            id: g.id,
            private_home: &gp.private_home,
            prefix: &gp.prefix,
            overlay_upper: &gp.overlay_upper,
            overlay_work: &gp.overlay_work,
            root: g.root.as_deref(),
            exe,
            umu_id: g.umu_id.as_deref(),
            store,
            prefix_lock: &gp.prefix_lock,
        }),
        _ => None,
    };
    let e = req.env;
    let ctx = SandboxContext {
        kind,
        uid: e.uid,
        gid: e.gid,
        user_name: e.user_name,
        group_name: e.group_name,
        home: e.home,
        game: game_ctx,
        runtime: req.runtime,
        proton_codename: req.proton_codename,
        session: &owned.session,
        paths,
        init_binary: e.init_binary,
        umu: umu_facts,
        network_override: req.network_override,
        launch: LaunchInputs {
            working_dir: &launch.working_dir,
            compat: &launch.compat,
            global_env: e.global_env,
            game_env: &launch.env,
            umu_args: &invocation.args,
            verbose_log: e.verbose_log,
        },
        host_env: e.host_env,
        install_id: e.install_id,
        developer_mode: e.developer_mode,
        seccomp_available: e.seccomp_available,
        user_dirs: e.user_dirs,
        other_game_roots: e.other_game_roots,
        mounts: e.mounts,
        fs: e.fs,
    };
    let policy = game.map_or(&owned.provision_policy, |g| &g.policy);
    Ok(f(&ctx, &invocation, policy))
}

/// Check that the sandbox plan runs exactly the umu invocation with its variables.
fn check_consistency(plan: &SandboxPlan, inv: &UmuInvocation) -> Result<(), LaunchPlanError> {
    let mut expected: Vec<std::ffi::OsString> = vec![
        INIT_PATH.into(),
        "--status-fd".into(),
        FD_INIT_STATUS.to_string().into(),
        "--".into(),
    ];
    expected.extend(inv.argv());
    if plan.command != expected {
        return Err(LaunchPlanError::Inconsistent(
            "the sandbox command differs from the umu invocation".into(),
        ));
    }
    for (k, v) in &inv.env {
        if plan.env.get(k) != Some(v.as_str()) {
            return Err(LaunchPlanError::Inconsistent(format!(
                "{k} differs between the sandbox environment and the umu invocation"
            )));
        }
    }
    Ok(())
}

/// Build the complete launch plan of `req`.
pub fn plan_launch(req: &PlanRequest<'_>) -> Result<PlannedLaunch, LaunchPlanError> {
    plan_launch_in(req, None)
}

/// Build the launch plan of a session that runs in the prefix at `prefix` (a Steam game's
/// `compatdata/<appid>`, ADR-0031 §6) instead of the game's own. Everything else — sandbox
/// sections, validation, the umu invocation — is [`plan_launch`]'s.
pub fn plan_launch_in_prefix(
    req: &PlanRequest<'_>,
    prefix: &Path,
) -> Result<PlannedLaunch, LaunchPlanError> {
    plan_launch_in(req, Some(prefix))
}

fn plan_launch_in(
    req: &PlanRequest<'_>,
    prefix: Option<&Path>,
) -> Result<PlannedLaunch, LaunchPlanError> {
    let Some(bwrap) = &req.host.bwrap else {
        return Err(LaunchPlanError::Host(ErrorCode::HostBwrapNotFound));
    };
    let kind = req.target.kind();
    let (planned, inv, policy) = with_context_in(req, prefix, |ctx, inv, policy| {
        (
            sandbox::plan(policy, ctx, req.host),
            inv.clone(),
            policy.clone(),
        )
    })?;
    let sandbox_plan = planned.map_err(LaunchPlanError::Sandbox)?;
    check_consistency(&sandbox_plan, &inv)?;
    let rendered = sandbox::render(&sandbox_plan)?;

    let paths = req.env.paths;
    let game = req.game.filter(|_| kind != SessionKind::Provision);
    let trusted_network = matches!(kind, SessionKind::Provision)
        || (kind == SessionKind::Prepare && req.network_override == Some(NetworkAccess::Full));
    let network = if sandbox_plan.namespaces.net {
        NetworkAccess::None
    } else {
        NetworkAccess::Full
    };
    let isolation = req
        .runtime
        .map(|r| r.isolation)
        .filter(|_| !trusted_network);
    let executable_id = match req.target {
        LaunchTarget::Game { exe, .. } | LaunchTarget::Installer { exe } => Some(exe.id),
        LaunchTarget::Tool(Tool::RunExe { path, .. }) => game
            .and_then(|g| g.executables.iter().find(|e| &e.canonical_path == path))
            .map(|e| e.id),
        _ => None,
    };
    let game_name = match game {
        Some(g) => g.name.clone(),
        None => "Compatibility layer download".to_owned(),
    };
    let plan = LaunchPlan {
        format: LAUNCH_PLAN_FORMAT,
        session: PlanSession {
            id: req.session_id,
            kind,
            game_id: game.map(|g| g.id),
            game_name,
            executable_id,
            counted_eligible: kind.may_count(),
            stop_grace_secs: game.map_or(LaunchConfig::default().stop_grace_secs, |g| {
                g.launch.stop_grace_secs
            }),
        },
        context: SessionContext {
            preset: game.map(|_| policy.preset).filter(|_| kind.is_untrusted()),
            network: Some(network),
            runtime_name: req.runtime.map(|r| r.name.clone()),
            slr_variant: req.runtime.map(|r| r.slr_variant),
            isolation,
        },
        bwrap: bwrap.path.clone(),
        sandbox: PlanSandbox {
            args: rendered.args.clone(),
            fds: rendered.fds.iter().map(PlanFd::from_payload).collect(),
            command: rendered.command.clone(),
        },
        host_actions: sandbox_plan.host_actions.clone(),
        paths: PlanPaths {
            log_dir: paths.session_dir(req.session_id),
            runtime_dir: paths.run_session_dir(req.session_id),
            session_var: (isolation == Some(RuntimeIsolationMode::Isolated))
                .then(|| paths.session_var_dir(req.session_id)),
            prefix_lock: game.map(|g| paths.prefix_lock(g.id)),
        },
        audit_after: sandbox_plan
            .audit_after
            .map(|variant| AuditAfter { variant }),
    };
    Ok(PlannedLaunch {
        plan,
        sandbox: sandbox_plan,
        umu: inv,
        rendered,
    })
}
