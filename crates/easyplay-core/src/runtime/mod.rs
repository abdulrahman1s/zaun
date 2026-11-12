//! Know which Proton builds and Steam Linux Runtimes exist, resolve a game's `RuntimeRef` to
//! concrete paths, describe provisioning work, and verify integrity.
//!
//! Layer 5. See MODULES §5 and RUNTIME_MANAGEMENT. Milestone M3 provides the store layout
//! ([`store`]), the VDF reader ([`vdf`]), discovery ([`discovery`]), the catalogue and its
//! synchronisation ([`catalog`]), the resolver ([`resolver`]) and the [`RuntimeService`]
//! skeleton. M7 adds integrity manifests ([`manifest`]), audits ([`audit`]) and their
//! scheduling and tamper handling ([`integrity`]), the Provision/Prepare decisions and
//! outcomes ([`provision`]), garbage collection ([`gc`]) and the fix database ([`umu_db`]).
//! The trusted sessions themselves are run by `launch::trusted` (this module never spawns).

pub mod adopt;
pub mod audit;
pub mod catalog;
pub mod discovery;
pub mod gc;
pub mod integrity;
pub mod manifest;
pub mod provision;
pub mod resolver;
pub mod service;
pub mod store;
pub mod umu_db;
pub mod vdf;

pub use adopt::{AdoptReport, adopt_slr, adoptable};
pub use audit::{AuditReport, TamperSummary, audit_variant};
pub use catalog::{CatalogDiff, CatalogStore, MemCatalogStore, RuntimeCatalog, SqliteCatalogStore};
pub use discovery::{Discovery, ProtonBuild, RejectReason, Rejected, SlrInstall, discover};
pub use gc::{KeepReason, RemovalCandidate};
pub use integrity::{AuditAction, AuditDue, IntegrityRecords, audit_due, handle_audit};
pub use provision::{NextStep, PrefixState, StepInput, TrustedEnd, next_step};
pub use resolver::{
    IsolationInputs, ProvisionNeed, Resolution, ResolveInput, UnavailableReason, resolve,
    select_isolation,
};
pub use service::{LocalRuntimeService, RuntimeSettings, catalog_check};
pub use store::{RuntimeStore, SandboxStore};
pub use umu_db::{UmuDb, UmuDbCache, UmuDbEntry};

use std::sync::Arc;

use crate::error::{Error, ErrorCode};
use crate::jobs::JobHandle;
use crate::model::{Game, RuntimeRef};

/// Errors of the runtime module.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// A build directory failed validation.
    #[error("{path}: {reason}")]
    Invalid {
        /// The directory.
        path: std::path::PathBuf,
        /// Why.
        reason: RejectReason,
    },
    /// The reference cannot be satisfied.
    #[error("compatibility layer unavailable: {0:?}")]
    Unavailable(UnavailableReason),
}

impl From<RuntimeError> for Error {
    fn from(e: RuntimeError) -> Self {
        match e {
            RuntimeError::Invalid { path, reason } => {
                let code = match reason {
                    RejectReason::HostRuntime | RejectReason::UnsupportedRuntime(_) => {
                        ErrorCode::RtUnsupportedRuntime
                    }
                    _ => ErrorCode::RtInvalidFolder,
                };
                Error::new(code)
                    .with("path", path.display())
                    .with("rule", reason.to_string())
            }
            RuntimeError::Unavailable(r) => Error::new(r.code()).with("reason", format!("{r:?}")),
        }
    }
}

/// The runtime façade (RUNTIME_MANAGEMENT §10). Milestone M3 implements catalogue access,
/// refresh and resolution; the provisioning, audit and removal methods join in M7.
pub trait RuntimeService: Send + Sync {
    /// The current snapshot.
    fn catalog(&self) -> Arc<RuntimeCatalog>;
    /// Rescan on the job pool and publish the new snapshot.
    fn refresh(&self) -> JobHandle<Arc<RuntimeCatalog>>;
    /// Resolve a reference, for `game` when given (follow-updates, last resolved, policy).
    fn resolve(&self, r: &RuntimeRef, game: Option<&Game>) -> Resolution;
}
