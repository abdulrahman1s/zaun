//! Turn a `SandboxPolicy` into the exact `bwrap` invocation, validate it, and explain it.
//!
//! Layer 4 (pure planning). See MODULES §5 and BUBBLEWRAP_SANDBOX.
//!
//! Pipeline: [`planner::plan`] (policy + [`SandboxContext`] + `HostFacts` → validated
//! [`SandboxPlan`]) → [`bwrap::render`] (arguments and descriptor payloads) and
//! [`explain::explain`] (access statements). Everything here is pure: the host is read only
//! through `HostFacts` and `Fs` reads; nothing is written or spawned ([`fd`] is used by the
//! spawner, not by the planner).

pub mod bwrap;
pub mod context;
pub mod explain;
pub mod fd;
pub mod plan;
pub mod planner;
pub mod presets;
pub mod seccomp;
pub mod sections;
pub mod validate;
pub mod wayland_ctx;

pub use bwrap::{RenderError, RenderedSandbox, display_lines, render};
pub use context::{GameContext, GamePaths, LaunchInputs, SandboxContext, SessionDirs};
pub use explain::{AccessCategory, AccessStatement, StatementSeverity, explain};
pub use plan::{
    EnvPlan, FdPayload, HostAction, Mount, MountKind, MountMode, MountOrigin, Namespaces,
    PlanBuilder, PolicyField, SandboxPlan, Section,
};
pub use planner::plan;
pub use presets::{Widening, apply_preset, classify, offline, online, standard, widening};
pub use validate::{
    IssueCode, IssueSeverity, PolicyIssue, ValidationInputs, plan_invariants, validate_policy,
};

use crate::error::{Error, ErrorCode};

/// The module's error type.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// Planning failed with blocking issues.
    #[error("sandbox planning failed: {}", .0.first().map_or("", |i| i.code.as_str()))]
    Plan(Vec<PolicyIssue>),
    /// Rendering failed.
    #[error(transparent)]
    Render(#[from] RenderError),
}

impl From<SandboxError> for Error {
    fn from(e: SandboxError) -> Self {
        match &e {
            SandboxError::Plan(issues) => {
                let first = issues
                    .iter()
                    .filter(|i| i.is_blocking())
                    .max_by_key(|i| i.severity)
                    .or(issues.first());
                let code = first.map_or(ErrorCode::SbxSetupFailed, |i| i.code.error_code());
                let mut err = Error::new(code);
                if let Some(i) = first {
                    err = err.with("issue", i.code.as_str());
                    if let Some(p) = &i.path {
                        err = err.with("path", p.display());
                    }
                }
                err.with_source(e)
            }
            SandboxError::Render(_) => Error::new(ErrorCode::SbxPlanInvariant).with_source(e),
        }
    }
}
