//! Discover facts about the machine once and expose them as immutable data.
//!
//! Layer 3. Owns the `HostFacts` cache (`<cache>/host-facts.json`). See MODULES §4 and
//! BUBBLEWRAP_SANDBOX §10. [`facts`] is pure data; each sibling file maps one probe onto it.
//! Every command a probe runs goes through [`CommandRunner`]; device access checks through
//! [`AccessCheck`]; everything else through `fsx::Fs`. Nothing here decides policy.

mod access;
pub mod audio;
pub mod bwrap_probe;
pub mod display;
pub mod doctor;
pub mod etc_links;
pub mod exec;
pub mod facts;
pub mod gpu;
pub mod input;
pub mod layout;
pub mod probe;
pub mod runner;
pub mod sysfs;
pub mod umu_probe;

pub use access::{AccessCheck, ModeAccess, RealAccess};
pub use doctor::{Check, CheckStatus, DOCTOR_FORMAT, DoctorContext, DoctorReport, host_checks};
pub use etc_links::{EtcLink, EtcLinkFacts};
pub use facts::{
    AudioFacts, BwrapFacts, BwrapFlag, DeviceNode, DisplayFacts, FsLayout, GpuFacts, GpuVendor,
    HOST_FACTS_FORMAT, HostEntry, HostFacts, HostHint, HostProblem, InputClass, InputFacts,
    InputNode, InputNodeKind, KernelFacts, LayoutKind, OsRelease, ResolverStrategy, SessionType,
    StoreFsFacts, SysfsFacts, ToolVersion, UMU_MIN_VERSION, UMU_VERIFIED_VERSION, UmuFacts,
    UmuWrapper, WaylandSocket, X11Display,
};
pub use probe::{HostProbe, LiveHostProbe, ProbeDeps, Revalidation, StaticHostFacts, probe_all};
pub use runner::{CommandOutput, CommandRunner, CommandSpec, RealRunner, RunError};

use crate::error::{Error, ErrorCode};

/// Errors of the host module.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// A probe command could not be run.
    #[error(transparent)]
    Run(#[from] RunError),
    /// A recorded host problem.
    #[error("{}: {}", .0.code, .0.detail)]
    Problem(HostProblem),
}

impl From<HostError> for Error {
    fn from(e: HostError) -> Self {
        match e {
            HostError::Run(r) => Error::new(ErrorCode::IntTaskFailed).with_source(r),
            HostError::Problem(p) => Error::new(p.code).with("detail", p.detail),
        }
    }
}
