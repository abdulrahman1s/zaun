//! The launch orchestrator (LAUNCH-004, LOGIC_SPECS §3): drives one launch from validation to
//! the hand-off to a supervisor, reporting each [`LaunchState`].
//!
//! Validating → ResolvingRuntime → (Provisioning | Preparing | AuditingRuntime)* → Planning →
//! Spawning → Starting. The trusted steps follow the decision table of RUNTIME_MANAGEMENT
//! §4.3 (`runtime::provision::next_step`) and are run through [`TrustedSteps`]; after each
//! one the catalogue is rescanned and the runtime resolved again. The audit gate (§6.3)
//! runs before planning; a quarantined runtime sends the launch back to ResolvingRuntime,
//! which then provisions it again. Without [`OrchestratorDeps::steps`] (the M6 behaviour)
//! a missing runtime ends the launch with [`LaunchError::NeedsProvision`] and a missing
//! prefix is created by the game session itself.
//!
//! Prompts raised by preflight (`ExeChanged`, `GrantTargetChanged`) are returned as
//! [`LaunchError::Prompt`]; a headless caller without a terminal fails with the prompt's
//! description (CONFIGURATION §6), the GUI flow re-enters after the answer.
//!
//! [`dry_run`] goes through the same decisions and planning without any session, host
//! action or write (LAUNCH-005).
//!
//! The orchestrator never waits for the game: from `Starting` on, the session belongs to
//! the supervisor and is observed through its row.

use std::sync::Arc;

use crate::clock::Clock;
use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::host::HostFacts;
use crate::ids::{ExecutableId, LaunchId, SessionId};
use crate::jobs::CancelToken;
use crate::model::{
    Game, NetworkAccess, ResolvedRuntime, RuntimeIsolationMode, RuntimeOrigin, SessionKind,
};
use crate::runtime::integrity::gate_applies;
use crate::runtime::provision::{NextStep, PrefixState, StepInput, interpret, next_step};
use crate::runtime::{AuditAction, ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use crate::sandbox::ValidationInputs;
use crate::session::{SessionHandle, SessionLauncher};
use crate::umu::UmuPhase;

use super::planner::{
    HostEnv, LaunchTarget, PlanRequest, PlannedLaunch, plan_launch, with_context,
};
use super::preflight::MIN_FREE_PROVISION_STORE;
use super::preflight::{
    FingerprintCheck, GrantChange, PrefixLocks, PreflightInput, PreflightReport, STAT_DEADLINE,
    preflight,
};
use super::state::{Launch, LaunchState};
use super::trusted::{TrustedProgress, TrustedSteps};

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
    /// The user allowed the network once for a trusted step on a tainted prefix (prompt
    /// `FixNeedsNetwork`, SECURITY_MODEL §4.1 rule 2).
    pub network_consent: bool,
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
    /// Provision, Prepare and the audit gate; `None`: stop at `NeedsProvision` (M6).
    pub steps: Option<&'a dyn TrustedSteps>,
    /// Cancels a waiting or running trusted step.
    pub cancel: Option<&'a CancelToken>,
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
    /// A Provision or Prepare session must run first and no [`TrustedSteps`] were given.
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

/// A trusted session the launch ran before the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustedRun {
    /// `Provision` or `Prepare`.
    pub kind: SessionKind,
    /// Its session.
    pub session: SessionId,
    /// Its network.
    pub network: NetworkAccess,
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
    /// Provision and Prepare sessions run first, in order.
    pub trusted: Vec<TrustedRun>,
    /// Notices raised on the way (`EP-RT-020`/`021` after a quarantine).
    pub notices: Vec<Error>,
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

/// Trusted steps a launch may take before giving up (a download that never satisfies the
/// resolver must not loop).
const MAX_TRUSTED_STEPS: usize = 4;

/// The pieces shared by [`launch`] and [`dry_run`].
struct Flow<'r, 'd> {
    req: &'r LaunchRequest<'r>,
    deps: &'r OrchestratorDeps<'d>,
    exe: &'r crate::model::Executable,
}

impl Flow<'_, '_> {
    fn plan_req<'p>(
        &'p self,
        session_id: SessionId,
        target: LaunchTarget<'p>,
        runtime: Option<&'p ResolvedRuntime>,
        codename: Option<&'p str>,
        network: Option<NetworkAccess>,
    ) -> PlanRequest<'p> {
        PlanRequest {
            session_id,
            target,
            game: Some(self.req.game),
            runtime,
            proton_codename: codename,
            network_override: network,
            host: self.deps.host,
            env: self.deps.env,
        }
    }

    fn game_target(&self) -> LaunchTarget<'_> {
        LaunchTarget::Game {
            exe: self.exe,
            extra_args: self.req.extra_args,
        }
    }

    /// Preflight with the context of `runtime`; without one only the prefix lock.
    fn preflight(&self, runtime: Option<&ResolvedRuntime>) -> Result<PreflightReport, LaunchError> {
        let deps = self.deps;
        let game = self.req.game;
        let Some(rt) = runtime else {
            if deps.locks.is_held(&deps.env.paths.prefix_lock(game.id)) {
                return Err(Error::new(ErrorCode::SesAlreadyRunning).into());
            }
            return Ok(PreflightReport {
                fingerprint: FingerprintCheck::Unchanged,
                grant_changes: Vec::new(),
                warnings: Vec::new(),
                low_space: None,
            });
        };
        let plan_req = self.plan_req(
            self.req.session_id,
            self.game_target(),
            Some(rt),
            None,
            None,
        );
        let prefix_lock = deps.env.paths.prefix_lock(game.id);
        let checked = with_context(&plan_req, |ctx, _, _| {
            preflight(&PreflightInput {
                game,
                exe: self.exe,
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
        let report = checked.map_err(Error::from)?;
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
        Ok(report)
    }

    fn resolve(&self) -> Resolution {
        self.deps
            .runtimes
            .resolve(&self.req.game.runtime, Some(self.req.game))
    }

    fn refresh(&self) -> Arc<RuntimeCatalog> {
        match self.deps.runtimes.refresh().wait() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "runtime rescan failed");
                self.deps.runtimes.catalog()
            }
        }
    }

    /// The decision for `resolution`, or `None` for "the game, without trusted steps".
    fn decide(&self, resolution: &Resolution) -> NextStep {
        let game = self.req.game;
        let Some(steps) = self.deps.steps else {
            return match resolution {
                Resolution::Ready(_) => NextStep::Game,
                Resolution::NeedsProvision(n) => NextStep::Provision(n.clone()),
                Resolution::Unavailable(r) => NextStep::Unavailable(r.clone()),
            };
        };
        let slr_build_external = match resolution {
            Resolution::NeedsProvision(ProvisionNeed::Slr { build, .. }) => self
                .deps
                .runtimes
                .catalog()
                .by_name(build)
                .is_none_or(|b| b.origin == RuntimeOrigin::External),
            _ => false,
        };
        next_step(&StepInput {
            resolution,
            prefix: PrefixState::read(
                &*self.deps.fs,
                &self.deps.env.paths.prefix_dir(game.id),
                game.prefix_tainted_at.is_some(),
            ),
            prepare_online: steps.prepare_online(),
            game_network: game.policy.network,
            consent: self.req.network_consent,
            slr_build_external,
        })
    }

    fn provision_plan(&self, need: &ProvisionNeed, sid: SessionId) -> Result<PlannedLaunch, Error> {
        plan_provision(
            need,
            &self.deps.runtimes.catalog(),
            self.deps.host,
            self.deps.env,
            sid,
        )
    }

    fn prepare_plan(
        &self,
        resolution: &Resolution,
        network: NetworkAccess,
        codename: Option<&str>,
        sid: SessionId,
    ) -> Result<PlannedLaunch, Error> {
        let rt = match resolution {
            Resolution::Ready(rt) => Some(rt),
            _ => None,
        };
        let req = self.plan_req(sid, LaunchTarget::Prepare, rt, codename, Some(network));
        Ok(plan_launch(&req)?)
    }

    fn check_space(&self) -> Result<(), Error> {
        let root = &self.deps.env.paths.data_dir;
        match self.deps.fs.free_space(root) {
            Ok(free) if free < MIN_FREE_PROVISION_STORE => Err(Error::new(ErrorCode::FsDiskFull)
                .with("path", root.display())
                .with("free_mib", free >> 20)
                .with("needed_mib", MIN_FREE_PROVISION_STORE >> 20)),
            _ => Ok(()),
        }
    }
}

/// The Provision plan for `need` (RUNTIME_MANAGEMENT §4.1). An external build that needs a
/// runtime is passed by path (mounted read-only); everything else by name, so umu
/// downloads what is missing.
pub fn plan_provision(
    need: &ProvisionNeed,
    catalog: &RuntimeCatalog,
    host: &HostFacts,
    env: &HostEnv<'_>,
    sid: SessionId,
) -> Result<PlannedLaunch, Error> {
    let external = match need {
        ProvisionNeed::Slr { variant, build } => catalog
            .by_name(build)
            .filter(|b| b.origin == RuntimeOrigin::External)
            .map(|b| ResolvedRuntime {
                name: b.name.clone(),
                proton_path_host: b.path.clone(),
                proton_path_sandbox: b.path.clone(),
                origin: RuntimeOrigin::External,
                slr_variant: *variant,
                slr_dir: env.paths.umu_store_dir.join(variant.as_str()),
                platform_dir: std::path::PathBuf::new(),
                isolation: RuntimeIsolationMode::Shared,
            }),
        _ => None,
    };
    let codename = need.protonpath();
    let req = PlanRequest {
        session_id: sid,
        target: LaunchTarget::Provision,
        game: None,
        runtime: external.as_ref(),
        proton_codename: external.is_none().then_some(codename.as_str()),
        network_override: Some(NetworkAccess::Full),
        host,
        env,
    };
    Ok(plan_launch(&req)?)
}

fn is_download(p: &UmuPhase) -> bool {
    matches!(
        p,
        UmuPhase::DownloadingProton { .. }
            | UmuPhase::DownloadingRuntime { .. }
            | UmuPhase::ResumingDownload { .. }
            | UmuPhase::Verified { .. }
            | UmuPhase::Extracting { .. }
            | UmuPhase::VerifyingRuntime
            | UmuPhase::RuntimeVerified
    )
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
    let flow = Flow { req, deps, exe };
    let no_cancel = CancelToken::new();
    let cancel = deps.cancel.unwrap_or(&no_cancel);

    // Validating: resolution is pure (catalogue snapshot); preflight needs its result.
    let mut resolution = flow.resolve();
    let first_rt = match &resolution {
        Resolution::Ready(rt) => Some(rt.clone()),
        _ => None,
    };
    let mut report = flow.preflight(first_rt.as_ref())?;
    let mut checked_with_runtime = first_rt.is_some();

    advance(&mut state, LaunchState::ResolvingRuntime, clock, on_state);
    let mut trusted: Vec<TrustedRun> = Vec::new();
    let mut notices: Vec<Error> = Vec::new();
    let mut prepared = false;
    let mut audited = false;
    let runtime = loop {
        if cancel.is_cancelled() {
            advance(&mut state, LaunchState::Cancelled, clock, on_state);
            return Err(Error::new(ErrorCode::IntTaskFailed)
                .with("reason", "cancelled")
                .into());
        }
        let step = flow.decide(&resolution);
        let steps = match (&step, deps.steps) {
            (NextStep::Unavailable(r), _) => {
                return Err(Error::new(r.code()).with("reason", format!("{r:?}")).into());
            }
            (NextStep::Game, steps) => {
                let Resolution::Ready(rt) = &resolution else {
                    unreachable_ready();
                    return Err(Error::new(ErrorCode::IntTaskFailed).into());
                };
                // The audit gate (§6.3).
                if let Some(steps) = steps
                    && !audited
                    && gate_applies(steps.audit_due(rt.slr_variant), rt.isolation)
                {
                    audited = true;
                    advance(&mut state, LaunchState::AuditingRuntime, clock, on_state);
                    let action = steps.audit(rt.slr_variant, cancel)?;
                    if let Some(n) = action.notice() {
                        notices.push(n);
                    }
                    if matches!(
                        action,
                        AuditAction::Quarantined { .. } | AuditAction::Missing
                    ) {
                        flow.refresh();
                        resolution = flow.resolve();
                        advance(&mut state, LaunchState::ResolvingRuntime, clock, on_state);
                        continue;
                    }
                }
                if prepared
                    && let Some(steps) = steps
                    && let Err(e) = steps.prepared(game.id, &rt.name)
                {
                    tracing::warn!(error = %e, "prefix_prepared_with not recorded");
                }
                break rt.clone();
            }
            (NextStep::Provision(need), None) => {
                return Err(LaunchError::NeedsProvision(need.clone()));
            }
            (_, Some(steps)) => steps,
            (NextStep::Prepare { .. }, None) => {
                // Not reached: without steps `decide` never asks for a Prepare.
                return Err(Error::new(ErrorCode::IntTaskFailed).into());
            }
        };
        if trusted.len() >= MAX_TRUSTED_STEPS {
            let need = match &resolution {
                Resolution::NeedsProvision(n) => n.protonpath(),
                _ => String::new(),
            };
            return Err(Error::new(ErrorCode::RtNotInstalled)
                .with("protonpath", need)
                .with(
                    "detail",
                    "the trusted steps did not complete the installation",
                )
                .into());
        }
        let sid = SessionId::generate(clock);
        match step {
            NextStep::Provision(need) => {
                flow.check_space()?;
                advance(
                    &mut state,
                    LaunchState::Provisioning {
                        phase: None,
                        bytes: 0,
                    },
                    clock,
                    on_state,
                );
                let planned = flow.provision_plan(&need, sid)?;
                let mut phase: Option<UmuPhase> = None;
                let mut bytes = 0u64;
                let end = steps.run(
                    &planned,
                    true,
                    &mut |p| {
                        match p {
                            TrustedProgress::Phase(p) => phase = Some(p),
                            TrustedProgress::Bytes { bytes: b, .. } => bytes = b,
                            TrustedProgress::Waiting => {}
                        }
                        advance(
                            &mut state,
                            LaunchState::Provisioning {
                                phase: phase.clone(),
                                bytes,
                            },
                            clock,
                            on_state,
                        );
                    },
                    cancel,
                )?;
                trusted.push(TrustedRun {
                    kind: SessionKind::Provision,
                    session: sid,
                    network: NetworkAccess::Full,
                });
                steps.after_install()?;
                interpret(&end)?;
                flow.refresh();
                // Post-condition (§4.1): the need is satisfied now; never download in a loop.
                resolution = flow.resolve();
                if let Resolution::NeedsProvision(still) = &resolution
                    && *still == need
                {
                    return Err(Error::new(ErrorCode::RtNotInstalled)
                        .with("protonpath", need.protonpath())
                        .with("detail", "the download did not install it")
                        .into());
                }
                advance(&mut state, LaunchState::ResolvingRuntime, clock, on_state);
                continue;
            }
            NextStep::Prepare { network, codename } => {
                if network == NetworkAccess::Full {
                    flow.check_space()?;
                }
                let downloading = codename.is_some();
                let first = if downloading {
                    LaunchState::Provisioning {
                        phase: None,
                        bytes: 0,
                    }
                } else {
                    LaunchState::Preparing { phase: None }
                };
                advance(&mut state, first, clock, on_state);
                let planned = flow.prepare_plan(&resolution, network, codename.as_deref(), sid)?;
                let mut bytes = 0u64;
                let mut in_prepare = !downloading;
                let end = steps.run(
                    &planned,
                    false,
                    &mut |p| {
                        let phase = match p {
                            TrustedProgress::Phase(p) => Some(p),
                            TrustedProgress::Bytes { bytes: b, .. } => {
                                bytes = b;
                                None
                            }
                            TrustedProgress::Waiting => None,
                        };
                        if let Some(p) = &phase
                            && !is_download(p)
                            && matches!(p, UmuPhase::Launching { .. } | UmuPhase::UpgradingPrefix)
                        {
                            in_prepare = true;
                        }
                        let next = if in_prepare {
                            LaunchState::Preparing { phase }
                        } else {
                            LaunchState::Provisioning { phase, bytes }
                        };
                        advance(&mut state, next, clock, on_state);
                    },
                    cancel,
                )?;
                trusted.push(TrustedRun {
                    kind: SessionKind::Prepare,
                    session: sid,
                    network,
                });
                // What a trusted session installed is recorded even if the Prepare failed.
                if network == NetworkAccess::Full {
                    steps.after_install()?;
                }
                if let Err(e) = interpret(&end) {
                    return Err(LaunchError::Failed(e));
                }
                prepared = true;
                flow.refresh();
            }
            NextStep::Game | NextStep::Unavailable(_) => {}
        }
        resolution = flow.resolve();
        advance(&mut state, LaunchState::ResolvingRuntime, clock, on_state);
    };

    // A launch that started without a runtime is checked in full now.
    if !checked_with_runtime {
        report = flow.preflight(Some(&runtime))?;
        checked_with_runtime = true;
    }
    debug_assert!(checked_with_runtime);

    // Planning.
    advance(&mut state, LaunchState::Planning, clock, on_state);
    let plan_req = flow.plan_req(
        req.session_id,
        flow.game_target(),
        Some(&runtime),
        None,
        None,
    );
    let planned = plan_launch(&plan_req).map_err(Error::from)?;

    // Spawning.
    advance(&mut state, LaunchState::Spawning, clock, on_state);
    let handle = deps.launcher.start(&planned.plan)?;
    if let Some(steps) = deps.steps
        && let Err(e) = steps.launched(game.id, &runtime.name)
    {
        tracing::warn!(error = %e, "runtime_last_resolved not recorded");
    }
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
        trusted,
        notices,
    })
}

#[cold]
fn unreachable_ready() {
    tracing::error!("next_step returned Game without a ready runtime");
}

/// Which session a dry run planned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DryStep {
    /// The game itself.
    Game,
    /// A Provision session would run first.
    Provision,
    /// A Prepare session would run first.
    Prepare,
}

/// The result of [`dry_run`].
#[derive(Debug)]
pub struct DryRun {
    /// The session that would run next.
    pub step: DryStep,
    /// Its plan.
    pub planned: PlannedLaunch,
    /// Why this step comes first (`None` for the game).
    pub note: Option<String>,
    /// An audit would run before the game (§6.3).
    pub audit_first: bool,
    /// Preflight findings (only with a ready runtime).
    pub report: Option<PreflightReport>,
}

/// LAUNCH-005: go through validation, resolution, the step decision and planning without
/// starting anything, executing host actions, or writing to the database or the disk.
/// When a Provision or Prepare would be needed, that step's plan is returned with a note.
pub fn dry_run(
    req: &LaunchRequest<'_>,
    deps: &OrchestratorDeps<'_>,
) -> Result<DryRun, LaunchError> {
    let game = req.game;
    let exe = match req.exe {
        Some(id) => game.executables.iter().find(|e| e.id == id),
        None => game.primary(),
    }
    .ok_or_else(|| {
        Error::new(ErrorCode::LibInvalidField).with("detail", "the game has no such executable")
    })?;
    let flow = Flow { req, deps, exe };
    let resolution = flow.resolve();
    let rt = match &resolution {
        Resolution::Ready(rt) => Some(rt),
        _ => None,
    };
    let report = match rt {
        Some(rt) => Some(flow.preflight(Some(rt))?),
        None => None,
    };
    let sid = req.session_id;
    let (step, planned, note) = match flow.decide(&resolution) {
        NextStep::Unavailable(r) => {
            return Err(Error::new(r.code()).with("reason", format!("{r:?}")).into());
        }
        NextStep::Game => {
            let Some(rt) = rt else {
                return Err(Error::new(ErrorCode::IntTaskFailed).into());
            };
            let r = flow.plan_req(sid, flow.game_target(), Some(rt), None, None);
            (DryStep::Game, plan_launch(&r).map_err(Error::from)?, None)
        }
        NextStep::Provision(need) => (
            DryStep::Provision,
            flow.provision_plan(&need, sid)?,
            Some(format!(
                "the compatibility layer must be downloaded first ({})",
                need.protonpath()
            )),
        ),
        NextStep::Prepare { network, codename } => (
            DryStep::Prepare,
            flow.prepare_plan(&resolution, network, codename.as_deref(), sid)?,
            Some(match (&codename, network) {
                (Some(c), _) => format!(
                    "the private Windows storage is created first; {c} is downloaded in the same step"
                ),
                (None, NetworkAccess::Full) => {
                    "the private Windows storage is created first (with internet access)".into()
                }
                (None, NetworkAccess::None) => {
                    "the private Windows storage is created first (offline)".into()
                }
            }),
        ),
    };
    let audit_first = match (deps.steps, rt) {
        (Some(s), Some(rt)) => gate_applies(s.audit_due(rt.slr_variant), rt.isolation),
        _ => false,
    };
    Ok(DryRun {
        step,
        planned,
        note,
        audit_first,
        report,
    })
}
