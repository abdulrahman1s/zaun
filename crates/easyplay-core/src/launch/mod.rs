//! The launch pipeline up to hand-off: validation, runtime resolution, plan assembly, state
//! reporting.
//!
//! Layer 5. See MODULES §6 and LOGIC_SPECS §3. Milestone M5 provides the states and timeline
//! ([`state`]), preflight ([`preflight`]), the planner for every session kind ([`planner`]),
//! the plan file ([`plan_file`]) and the reproducible shell rendering ([`shell`]). M6 adds
//! the [`orchestrator`] up to the hand-off to a supervisor (without the Provision, Prepare
//! and audit branches, which follow in M7 with dry runs through the service, LAUNCH-005).

pub mod orchestrator;
pub mod plan_file;
pub mod planner;
pub mod preflight;
pub mod shell;
pub mod state;

pub use orchestrator::{
    LaunchError, LaunchOutcome, LaunchPrompt, LaunchRequest, OrchestratorDeps, launch,
};
pub use plan_file::{
    AuditAfter, FdKind, LAUNCH_PLAN_FORMAT, LaunchPlan, PlanFd, PlanFileError, PlanPaths,
    PlanSandbox, PlanSession,
};
pub use planner::{
    HostEnv, LaunchPlanError, LaunchTarget, PlanRequest, PlannedLaunch, plan_launch, with_context,
};
pub use preflight::{
    FingerprintCheck, GrantChange, NoLocks, PrefixLocks, PreflightFailure, PreflightInput,
    PreflightReport, preflight,
};
pub use shell::render_shell;
pub use state::{IllegalTransition, Launch, LaunchState, LaunchTiming, Mark, StateKind, Timeline};
