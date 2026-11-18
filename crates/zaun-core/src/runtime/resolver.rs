//! `RuntimeRef` → [`Resolution`] (RUNTIME_MANAGEMENT §3, isolation mode §7). Pure: reads only
//! the catalogue snapshot and the inputs; external directories not in the catalogue are
//! validated by the caller beforehand (`RuntimeService::resolve`).

use std::path::{Path, PathBuf};

use super::catalog::RuntimeCatalog;
use super::store::{RuntimeStore, SandboxStore, is_downloadable_name};
use crate::error::ErrorCode;
use crate::ids::RuntimeName;
use crate::model::{
    Channel, ResolvedRuntime, RuntimeBuild, RuntimeIsolation, RuntimeIsolationMode, RuntimeOrigin,
    RuntimeRef, SlrVariant,
};

/// What must be installed before the launch can proceed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProvisionNeed {
    /// The newest build of a channel (`PROTONPATH=<codename>`).
    Codename(Channel),
    /// An exact build (`PROTONPATH=<name>`).
    Exact(RuntimeName),
    /// The SLR variant required by an installed build.
    Slr {
        /// Variant to install.
        variant: SlrVariant,
        /// The build that needs it.
        build: RuntimeName,
    },
}

impl ProvisionNeed {
    /// The `PROTONPATH` value a Provision session uses.
    pub fn protonpath(&self) -> String {
        match self {
            ProvisionNeed::Codename(c) => c.umu_codename().to_owned(),
            ProvisionNeed::Exact(n) | ProvisionNeed::Slr { build: n, .. } => n.to_string(),
        }
    }
}

/// Why a reference cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    /// A pinned build that is not installed and that umu cannot download.
    UnknownPinned(RuntimeName),
    /// An external directory that is gone or no longer valid.
    ExternalInvalid(PathBuf),
    /// The build requires no container runtime, or one unsupported in 1.0.
    UnsupportedRuntime(RuntimeName),
}

impl UnavailableReason {
    /// The error code shown to the user.
    pub fn code(&self) -> ErrorCode {
        match self {
            UnavailableReason::UnknownPinned(_) | UnavailableReason::ExternalInvalid(_) => {
                ErrorCode::RtNoLongerInstalled
            }
            UnavailableReason::UnsupportedRuntime(_) => ErrorCode::RtUnsupportedRuntime,
        }
    }
}

/// The outcome of resolving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Everything is installed.
    Ready(ResolvedRuntime),
    /// Something must be installed first.
    NeedsProvision(ProvisionNeed),
    /// The reference cannot be satisfied.
    Unavailable(UnavailableReason),
}

/// Inputs of the isolation choice (§7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsolationInputs {
    /// `policy.hardening.runtime_isolation`.
    pub policy: RuntimeIsolation,
    /// Preference `protection.always_isolate_runtime`.
    pub always_isolate: bool,
    /// `HostFacts.fs.reflink_ok` for the store's device.
    pub reflink_ok: Option<bool>,
}

impl Default for IsolationInputs {
    fn default() -> Self {
        Self {
            policy: RuntimeIsolation::Auto,
            always_isolate: false,
            reflink_ok: None,
        }
    }
}

/// §7: Isolated / Shared per launch.
pub fn select_isolation(i: IsolationInputs) -> RuntimeIsolationMode {
    match i.policy {
        RuntimeIsolation::Isolated => RuntimeIsolationMode::Isolated,
        RuntimeIsolation::Shared => RuntimeIsolationMode::Shared,
        RuntimeIsolation::Auto if i.always_isolate || i.reflink_ok == Some(true) => {
            RuntimeIsolationMode::Isolated
        }
        RuntimeIsolation::Auto => RuntimeIsolationMode::Shared,
    }
}

/// Everything besides the catalogue.
#[derive(Clone, Debug)]
pub struct ResolveInput<'a> {
    /// The game's reference.
    pub reference: &'a RuntimeRef,
    /// Preference `runtime.default` (initially `Channel(UmuProton)`).
    pub default: &'a RuntimeRef,
    /// `games.runtime_follow_updates`.
    pub follow_updates: bool,
    /// `games.runtime_last_resolved`.
    pub last_resolved: Option<&'a RuntimeName>,
    /// The store.
    pub store: &'a RuntimeStore,
    /// The real home path (sandbox-side locations live under it).
    pub home: &'a Path,
    /// Isolation inputs.
    pub isolation: IsolationInputs,
}

/// Why no build was picked.
enum Miss {
    Provision(ProvisionNeed),
    Unavailable(UnavailableReason),
}

fn pick<'c>(
    r: &RuntimeRef,
    input: &ResolveInput<'_>,
    catalog: &'c RuntimeCatalog,
    depth: u8,
) -> Result<&'c RuntimeBuild, Miss> {
    match r {
        RuntimeRef::Default if depth == 0 => pick(input.default, input, catalog, 1),
        RuntimeRef::Default => pick(&RuntimeRef::Channel(Channel::UmuProton), input, catalog, 2),
        RuntimeRef::Channel(c) => {
            if !input.follow_updates
                && let Some(b) = input
                    .last_resolved
                    .and_then(|n| catalog.by_name(n))
                    .filter(|b| b.family == c.family())
            {
                return Ok(b);
            }
            catalog
                .newest(c.family())
                .ok_or(Miss::Provision(ProvisionNeed::Codename(*c)))
        }
        RuntimeRef::Pinned(name) => catalog.by_name(name).ok_or_else(|| {
            if is_downloadable_name(name.as_str()) {
                Miss::Provision(ProvisionNeed::Exact(name.clone()))
            } else {
                Miss::Unavailable(UnavailableReason::UnknownPinned(name.clone()))
            }
        }),
        RuntimeRef::External(path) => catalog
            .by_path(path)
            .filter(|b| b.origin == RuntimeOrigin::External)
            .ok_or_else(|| Miss::Unavailable(UnavailableReason::ExternalInvalid(path.clone()))),
    }
}

/// Resolve a reference against a catalogue snapshot.
pub fn resolve(input: &ResolveInput<'_>, catalog: &RuntimeCatalog) -> Resolution {
    let build = match pick(input.reference, input, catalog, 0) {
        Ok(b) => b,
        Err(Miss::Provision(p)) => return Resolution::NeedsProvision(p),
        Err(Miss::Unavailable(u)) => return Resolution::Unavailable(u),
    };
    let variant = build.required_slr;
    let Some(slr) = catalog.slr(variant).filter(|s| s.is_complete()) else {
        return Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant,
            build: build.name.clone(),
        });
    };
    let Some(platform_dir) = slr.platform_dir.clone() else {
        return Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant,
            build: build.name.clone(),
        });
    };
    let sandbox = SandboxStore::new(input.home);
    let proton_path_sandbox = match build.origin {
        RuntimeOrigin::Store => sandbox.proton_dir(build.name.as_str()),
        RuntimeOrigin::External => build.path.clone(),
    };
    Resolution::Ready(ResolvedRuntime {
        name: build.name.clone(),
        proton_path_host: build.path.clone(),
        proton_path_sandbox,
        origin: build.origin,
        slr_variant: variant,
        slr_dir: input.store.slr_dir(variant),
        platform_dir,
        isolation: select_isolation(input.isolation),
    })
}
