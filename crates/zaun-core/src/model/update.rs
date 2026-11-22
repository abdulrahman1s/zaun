//! What Settings › About shows about updates (ADR-0040): how this copy was installed, the
//! last check, the newest verified version and the download.

use crate::clock::Timestamp;
use crate::error::ErrorCode;

/// How this copy of Zaun was installed, which decides who updates it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstallChannel {
    /// From the Nix store. `bundles` when the system can run the self-contained bundle
    /// (nix-ld, `programs.zaun.selfUpdate`); otherwise the flake input is updated.
    Nix { bundles: bool },
    /// A Flatpak: `flatpak update`.
    Flatpak,
    /// A distribution package (under `/usr`).
    Package,
    /// The release tarball's `install.sh`.
    Tarball,
    /// A version from the bundle store, started by the installed copy (ADR-0041).
    Bundle,
    /// Anything else (a development build, an unpacked tree): announce only.
    #[default]
    Other,
}

impl InstallChannel {
    /// Whether Zaun downloads new versions into its bundle store and starts them.
    pub fn self_updates(self) -> bool {
        match self {
            InstallChannel::Package | InstallChannel::Tarball | InstallChannel::Bundle => true,
            InstallChannel::Nix { bundles } => bundles,
            InstallChannel::Flatpak | InstallChannel::Other => false,
        }
    }
}

/// A newer version, from a verified release manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateOffer {
    pub version: String,
    /// Release date as the manifest gives it (`2026-12-01`).
    pub date: String,
    /// One paragraph, control characters removed, at most 280 characters.
    pub notes: String,
    /// The release page exists (opened with `UpdateCommand::OpenNotes`).
    pub has_notes: bool,
}

/// The download of a copy that updates itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateDownload {
    #[default]
    Idle,
    Downloading,
    /// Verified and waiting in the bundle store; started on restart.
    Staged {
        version: String,
    },
}

/// Updates as the UI shows them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateSnapshot {
    /// This build has an update key (otherwise it never checks).
    pub configured: bool,
    pub channel: InstallChannel,
    /// The running version.
    pub current: String,
    pub last_checked: Option<Timestamp>,
    pub checking: bool,
    /// A newer verified version, if the last check found one.
    pub available: Option<UpdateOffer>,
    pub download: UpdateDownload,
    /// Why the last check or download failed (until the next one succeeds).
    pub failure: Option<ErrorCode>,
    /// A version that did not start and was replaced by the previous one.
    pub rolled_back: Option<String>,
}
