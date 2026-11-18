//! Compatibility-layer references and resolved runtimes (DATA_MODEL §4, RUNTIME_MANAGEMENT §3).
//!
//! `ResolvedRuntime`, `SlrVariant` and `RuntimeIsolationMode` live here (not in `runtime`) so
//! that the sandbox planner (layer 4) never imports the runtime module (layer 5).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::clock::Timestamp;
use crate::ids::RuntimeName;

/// Which Proton build a game uses.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRef {
    /// Follow the launcher-wide default (a channel or a pinned build).
    Default,
    /// The newest installed build of a family.
    Channel(Channel),
    /// An exact build name in the store.
    Pinned(RuntimeName),
    /// An exact directory elsewhere.
    External(PathBuf),
}

/// A family of Proton builds that follows updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// UMU-Proton (umu's own default).
    UmuProton,
    /// GE-Proton.
    GeProton,
}

impl Channel {
    /// The codename umu accepts in `PROTONPATH` to download the newest build (F-104). The
    /// `*-Latest` delta-patch codenames are never used (ADR-0010).
    pub const fn umu_codename(self) -> &'static str {
        match self {
            Channel::UmuProton => "UMU-Proton",
            Channel::GeProton => "GE-Proton",
        }
    }

    /// The matching catalogue family.
    pub const fn family(self) -> RuntimeFamily {
        match self {
            Channel::UmuProton => RuntimeFamily::UmuProton,
            Channel::GeProton => RuntimeFamily::GeProton,
        }
    }
}

/// Family of a Proton build in the catalogue (`runtimes.family`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFamily {
    /// UMU-Proton.
    UmuProton,
    /// GE-Proton.
    GeProton,
    /// Valve's Proton.
    ValveProton,
    /// Anything else.
    Other,
}

impl RuntimeFamily {
    /// Text form for the `runtimes.family` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            RuntimeFamily::UmuProton => "umu_proton",
            RuntimeFamily::GeProton => "ge_proton",
            RuntimeFamily::ValveProton => "valve_proton",
            RuntimeFamily::Other => "other",
        }
    }
}

/// Where a Proton build lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOrigin {
    /// Zaun's runtime store.
    Store,
    /// Elsewhere on the machine, used in place, read-only.
    External,
}

impl RuntimeOrigin {
    /// Text form for the `runtimes.origin` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            RuntimeOrigin::Store => "store",
            RuntimeOrigin::External => "external",
        }
    }
}

/// A Steam Linux Runtime variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlrVariant {
    /// Steam Linux Runtime 3 "sniper".
    Steamrt3,
    /// Steam Linux Runtime 4.
    Steamrt4,
}

impl SlrVariant {
    /// Directory and column name, e.g. `steamrt3`.
    pub const fn as_str(self) -> &'static str {
        match self {
            SlrVariant::Steamrt3 => "steamrt3",
            SlrVariant::Steamrt4 => "steamrt4",
        }
    }

    /// Parse [`SlrVariant::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "steamrt3" => Some(SlrVariant::Steamrt3),
            "steamrt4" => Some(SlrVariant::Steamrt4),
            _ => None,
        }
    }
}

/// The isolation mode actually used for one launch (RUNTIME_MANAGEMENT §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeIsolationMode {
    /// A private copy per session (`session-var/<sid>`).
    Isolated,
    /// The audited shared runtime.
    Shared,
}

impl RuntimeIsolationMode {
    /// Text form for the `sessions.isolation` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            RuntimeIsolationMode::Isolated => "isolated",
            RuntimeIsolationMode::Shared => "shared",
        }
    }
}

/// A Proton build in the catalogue (a `runtimes` row).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBuild {
    /// Build name, e.g. `GE-Proton10-25`.
    pub name: RuntimeName,
    /// Family.
    pub family: RuntimeFamily,
    /// Zero-padded natural sort key.
    pub version_key: String,
    /// Store or external.
    pub origin: RuntimeOrigin,
    /// Human description of where it was found ("Zaun", "Steam", a folder name).
    pub origin_label: String,
    /// Directory of the build.
    pub path: PathBuf,
    /// The SLR this build requires.
    pub required_slr: SlrVariant,
    /// Name shown in the UI.
    pub display_name: String,
    /// Whether the build ships `umu-database.csv` / protonfixes.
    pub has_fixes: bool,
    /// Size on disk, when measured.
    pub size_bytes: Option<u64>,
    /// When it was first seen.
    pub discovered_at: Timestamp,
    /// When a game last used it.
    pub last_used_at: Option<Timestamp>,
}

/// A runtime reference resolved to concrete paths for one launch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRuntime {
    /// Build name.
    pub name: RuntimeName,
    /// Proton directory on the host.
    pub proton_path_host: PathBuf,
    /// Proton directory as seen inside the sandbox.
    pub proton_path_sandbox: PathBuf,
    /// Store or external.
    pub origin: RuntimeOrigin,
    /// Required SLR variant.
    pub slr_variant: SlrVariant,
    /// SLR directory on the host (`<store>/umu/<variant>`).
    pub slr_dir: PathBuf,
    /// The SLR's platform directory (e.g. `sniper_platform_<version>`).
    pub platform_dir: PathBuf,
    /// Isolation mode for this launch.
    pub isolation: RuntimeIsolationMode,
}
