//! Stable error codes `EP-AREA-NNN` (ERROR_HANDLING §3).
//!
//! The string form of a code never changes once released; codes are only added.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::user::Severity;

macro_rules! error_codes {
    ($( $(#[$meta:meta])* $variant:ident = $text:literal, $sev:ident; )*) => {
        /// Every error code EasyPlay can report. See ERROR_HANDLING §3 for meanings and
        /// primary actions; `catalog::describe` produces the user-facing text.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum ErrorCode {
            $( $(#[$meta])* $variant, )*
        }

        impl ErrorCode {
            /// Every code, in catalogue order.
            pub const ALL: &'static [ErrorCode] = &[ $( ErrorCode::$variant, )* ];

            /// The stable string form, e.g. `EP-HOST-001`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( ErrorCode::$variant => $text, )*
                }
            }

            /// The severity listed in the catalogue.
            pub const fn severity(self) -> Severity {
                match self {
                    $( ErrorCode::$variant => Severity::$sev, )*
                }
            }
        }
    };
}

error_codes! {
    // ---- Host — EP-HOST ------------------------------------------------------------------
    /// bubblewrap not found.
    HostBwrapNotFound = "EP-HOST-001", Error;
    /// bubblewrap lacks a required option.
    HostBwrapMissingOption = "EP-HOST-002", Error;
    /// bubblewrap is setuid-only and user namespaces are unavailable.
    HostBwrapSetuidOnly = "EP-HOST-003", Error;
    /// Unprivileged user namespaces unavailable.
    HostUsernsUnavailable = "EP-HOST-004", Error;
    /// Nested user namespaces unavailable.
    HostNestedUsernsUnavailable = "EP-HOST-005", Error;
    /// `easyplay-init` missing or not executable.
    HostInitMissing = "EP-HOST-006", Error;
    /// `umu-run` not found.
    HostUmuNotFound = "EP-HOST-010", Error;
    /// `umu-run` version too old.
    HostUmuTooOld = "EP-HOST-011", Error;
    /// `umu-run` installation cannot be made visible safely.
    HostUmuClosureRejected = "EP-HOST-012", Error;
    /// No display server socket available.
    HostNoDisplay = "EP-HOST-020", Error;
    /// No GPU render node accessible.
    HostNoGpu = "EP-HOST-021", Warning;
    /// Running as root is not supported.
    HostRunningAsRoot = "EP-HOST-030", Error;
    /// musl-based system not supported by umu.
    HostMuslUnsupported = "EP-HOST-031", Error;

    // ---- Sandbox — EP-SBX ----------------------------------------------------------------
    /// Sandbox could not be set up (unclassified bwrap error).
    SbxSetupFailed = "EP-SBX-001", Error;
    /// A permitted folder or file is missing.
    SbxGrantMissing = "EP-SBX-010", Error;
    /// Internal planning error: mount on a symlink.
    SbxMountOnSymlink = "EP-SBX-011", Error;
    /// Protected game folder (overlay) not supported here.
    SbxOverlayUnsupported = "EP-SBX-012", Error;
    /// Init shim not runnable inside the sandbox.
    SbxInitNotRunnable = "EP-SBX-013", Error;
    /// No way to show a window with the current display permissions.
    SbxNoDisplayAccess = "EP-SBX-020", Error;
    /// Internal planning error: runtime layout.
    SbxRuntimeLayout = "EP-SBX-030", Error;
    /// This game's permissions could not be read.
    SbxPolicyUnreadable = "EP-SBX-040", Error;
    /// A permission is not allowed (deny-list).
    SbxDenied = "EP-SBX-050", Error;
    /// A permission is unusually broad.
    SbxBroadGrant = "EP-SBX-051", Warning;
    /// A granted folder now points somewhere else.
    SbxGrantTargetChanged = "EP-SBX-052", Warning;
    /// Plan invariant violated (bug); fatal for the launch only.
    SbxPlanInvariant = "EP-SBX-900", Fatal;

    // ---- Runtime — EP-RT -----------------------------------------------------------------
    /// Compatibility layer not installed and no internet.
    RtNotInstalledOffline = "EP-RT-010", Error;
    /// Compatibility layer could not be resolved.
    RtUnresolvable = "EP-RT-011", Error;
    /// Compatibility layer folder is not valid.
    RtInvalidFolder = "EP-RT-012", Error;
    /// The layer needs a runtime this system setup does not support.
    RtUnsupportedRuntime = "EP-RT-013", Error;
    /// Download failed verification.
    RtVerificationFailed = "EP-RT-014", Error;
    /// The selected layer is no longer installed.
    RtNoLongerInstalled = "EP-RT-015", Error;
    /// Download interrupted.
    RtDownloadInterrupted = "EP-RT-016", Error;
    /// A compatibility layer or runtime must be installed and this build cannot install it
    /// (provisioning arrives in M7).
    RtNotInstalled = "EP-RT-017", Error;
    /// Shared runtime was modified and has been removed.
    RtSharedTampered = "EP-RT-020", Warning;
    /// Runtime files damaged (no game suspected).
    RtDamaged = "EP-RT-021", Warning;
    /// Not enough free space for a private runtime copy.
    RtNoSpaceForCopy = "EP-RT-030", Error;
    /// Private Windows storage could not be prepared.
    RtPrefixPrepareFailed = "EP-RT-040", Error;
    /// Private Windows storage appears damaged.
    RtPrefixDamaged = "EP-RT-041", Error;

    // ---- umu and container — EP-UMU ------------------------------------------------------
    /// The compatibility launcher stopped with an internal error.
    UmuInternalError = "EP-UMU-001", Error;
    /// The program did not start (exited at once with an error).
    UmuProgramDidNotStart = "EP-UMU-002", Error;
    /// The compatibility container could not start.
    UmuContainerFailed = "EP-UMU-003", Error;
    /// A game fix needs the internet.
    UmuFixNeedsNetwork = "EP-UMU-004", Warning;

    // ---- Session — EP-SES ----------------------------------------------------------------
    /// Background process for the game did not start.
    SesSupervisorNotStarted = "EP-SES-001", Error;
    /// Launch plan not understood (version mismatch after an update).
    SesPlanVersionMismatch = "EP-SES-002", Error;
    /// Already running.
    SesAlreadyRunning = "EP-SES-010", Info;
    /// The game did not stop cleanly; forced.
    SesForcedStop = "EP-SES-020", Warning;
    /// Not permitted to signal the session.
    SesSignalDenied = "EP-SES-021", Error;
    /// A previous session was interrupted and has been closed.
    SesInterrupted = "EP-SES-030", Warning;

    // ---- Game — EP-GAME ------------------------------------------------------------------
    /// The game closed right after starting.
    GameClosedEarly = "EP-GAME-001", Warning;
    /// The game crashed.
    GameCrashed = "EP-GAME-002", Warning;
    /// The game was stopped by the system (likely out of memory).
    GameKilledBySystem = "EP-GAME-003", Warning;

    // ---- Library and identity — EP-LIB, EP-ID --------------------------------------------
    /// Program file not found.
    LibExeNotFound = "EP-LIB-001", Error;
    /// The drive this game is on is not connected.
    LibDriveDisconnected = "EP-LIB-002", Error;
    /// Program file cannot be read (permissions).
    LibExeUnreadable = "EP-LIB-003", Error;
    /// This program is already in the library.
    LibAlreadyInLibrary = "EP-LIB-010", Info;
    /// Another action for this game is still in progress.
    LibBusy = "EP-LIB-020", Info;
    /// Name, tag or note not valid.
    LibInvalidField = "EP-LIB-030", Error;
    /// Not a Windows program EasyPlay can run.
    IdNotWindowsProgram = "EP-ID-001", Error;
    /// The program's information could not be read; using the file name.
    IdPeUnreadable = "EP-ID-002", Warning;

    // ---- Files — EP-FS -------------------------------------------------------------------
    /// Disk full.
    FsDiskFull = "EP-FS-001", Error;
    /// Low disk space before starting.
    FsLowSpace = "EP-FS-002", Warning;
    /// Could not write (permission denied).
    FsWriteDenied = "EP-FS-003", Error;
    /// Could not delete some files; will retry at next start.
    FsDeleteIncomplete = "EP-FS-004", Error;
    /// Cannot read the file (permission).
    FsReadDenied = "EP-FS-010", Error;
    /// The location did not respond in time.
    FsTimeout = "EP-FS-011", Error;

    // ---- Database and configuration — EP-DB, EP-CFG --------------------------------------
    /// Library file damaged.
    DbCorrupt = "EP-DB-001", Fatal;
    /// Library busy; change not saved.
    DbBusy = "EP-DB-002", Error;
    /// Library created by a newer EasyPlay.
    DbNewerVersion = "EP-DB-010", Fatal;
    /// Close running games before updating the library.
    DbSessionsRunning = "EP-DB-011", Error;
    /// Could not back up before updating; update not attempted.
    DbBackupFailed = "EP-DB-012", Fatal;
    /// Library update failed; previous version restored (recovery mode).
    DbMigrationFailed = "EP-DB-013", Fatal;
    /// A setting was not valid and was not changed.
    CfgInvalidValue = "EP-CFG-001", Warning;
    /// Settings file could not be read; defaults in use.
    CfgUnreadable = "EP-CFG-002", Warning;
    /// Settings file is from a newer version; using defaults without changing it.
    CfgNewerVersion = "EP-CFG-003", Warning;

    // ---- Artwork — EP-ART ----------------------------------------------------------------
    /// Image format not supported.
    ArtUnsupportedFormat = "EP-ART-001", Error;
    /// Image too large.
    ArtTooLarge = "EP-ART-002", Error;
    /// Image could not be read.
    ArtUnreadable = "EP-ART-003", Error;
    /// Steam has no official artwork for this game (or its app ID is unknown).
    ArtNotFound = "EP-ART-004", Warning;
    /// Steam could not be reached for artwork or a search.
    ArtNoConnection = "EP-ART-005", Warning;
    /// The program has no icon to use.
    ArtNoIcon = "EP-ART-006", Warning;

    // ---- Steam games — EP-STM (ADR-0031) -------------------------------------------------
    /// No Steam installation was found.
    StmNotFound = "EP-STM-001", Warning;
    /// Steam no longer lists this game (it was uninstalled, or its drive is not connected).
    StmGameGone = "EP-STM-002", Warning;
    /// Steam could not be asked (its program is missing or did not start).
    StmClientUnavailable = "EP-STM-003", Error;
    /// The game is running, so nothing can run in its Windows storage now.
    StmGameRunning = "EP-STM-004", Warning;
    /// The game has no Windows storage (not run through Proton, or not played yet).
    StmNoPrefix = "EP-STM-005", Warning;
    /// The compatibility layer Steam uses for the game cannot be used here.
    StmLayerUnavailable = "EP-STM-006", Error;
    /// The action does not apply to a game Steam runs.
    StmNotForSteam = "EP-STM-007", Info;
    /// Steam is open, so its settings file cannot be changed now (Steam rewrites it when it
    /// closes).
    StmSteamOpen = "EP-STM-008", Warning;
    /// Steam's settings file for the account could not be found, read safely or written.
    StmSettingsUnwritable = "EP-STM-009", Error;

    // ---- Friends — EP-FRN (ADR-0033) ------------------------------------------------------
    /// This build of EasyPlay has no chat engine (the Friends helper or its library is
    /// missing).
    FrnUnavailable = "EP-FRN-001", Warning;
    /// The chat engine stopped again and again, so Friends was stopped.
    FrnEngineFailing = "EP-FRN-002", Error;
    /// The encrypted chat database could not be opened.
    FrnDatabase = "EP-FRN-003", Error;
    /// No relay could be reached (no connection, or the relays are down).
    FrnNoConnection = "EP-FRN-004", Warning;
    /// The text is not a SimpleX invitation or address link.
    FrnBadLink = "EP-FRN-005", Warning;
    /// The chat engine refused a request.
    FrnRequestFailed = "EP-FRN-006", Warning;

    // ---- Integration and internal — EP-DSK, EP-INT ---------------------------------------
    /// Could not change the default application.
    DskDefaultHandlerFailed = "EP-DSK-001", Error;
    /// Could not create the shortcut.
    DskShortcutFailed = "EP-DSK-002", Error;
    /// Another EasyPlay instance did not respond.
    DskInstanceUnresponsive = "EP-DSK-003", Warning;
    /// An internal task failed unexpectedly.
    IntTaskFailed = "EP-INT-001", Error;
    /// EasyPlay has to close (unrecoverable internal error).
    IntFatal = "EP-INT-002", Fatal;
    /// This feature is not available yet in this build (placeholder for a later milestone;
    /// the context carries the tracking task ID).
    IntNotAvailable = "EP-INT-003", Info;
}

impl ErrorCode {
    /// The area part, e.g. `HOST` for `EP-HOST-001`.
    pub fn area(self) -> &'static str {
        let s = self.as_str();
        let rest = &s[3..];
        match rest.find('-') {
            Some(i) => &rest[..i],
            None => rest,
        }
    }

    /// The numeric part, e.g. `1` for `EP-HOST-001`.
    pub fn number(self) -> u16 {
        let s = self.as_str();
        s.rsplit('-')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An unknown error-code string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown error code: {0:?}")]
pub struct UnknownErrorCode(pub String);

impl FromStr for ErrorCode {
    type Err = UnknownErrorCode;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ErrorCode::ALL
            .iter()
            .copied()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| UnknownErrorCode(s.to_owned()))
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn code_strings_are_unique_and_well_formed() {
        let mut seen = HashSet::new();
        for code in ErrorCode::ALL {
            let s = code.as_str();
            assert!(seen.insert(s), "duplicate {s}");
            let parts: Vec<&str> = s.split('-').collect();
            assert_eq!(parts.len(), 3, "{s}");
            assert_eq!(parts[0], "EP", "{s}");
            assert!(
                !parts[1].is_empty() && parts[1].bytes().all(|b| b.is_ascii_uppercase()),
                "{s}"
            );
            assert_eq!(parts[2].len(), 3, "{s}");
            assert!(parts[2].bytes().all(|b| b.is_ascii_digit()), "{s}");
            assert_eq!(code.area(), parts[1]);
        }
    }

    #[test]
    fn codes_parse_and_serialise() {
        for code in ErrorCode::ALL {
            assert_eq!(code.as_str().parse::<ErrorCode>(), Ok(*code));
            let json = serde_json::to_string(code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
            assert_eq!(serde_json::from_str::<ErrorCode>(&json).unwrap(), *code);
        }
        assert!("EP-NOPE-001".parse::<ErrorCode>().is_err());
        assert_eq!(ErrorCode::SbxPlanInvariant.number(), 900);
        assert_eq!(ErrorCode::LibInvalidField.area(), "LIB");
    }
}
