//! Stable error codes `ZN-AREA-NNN` (ERROR_HANDLING §3).
//!
//! The string form of a code never changes once released; codes are only added.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::user::Severity;

macro_rules! error_codes {
    ($( $(#[$meta:meta])* $variant:ident = $text:literal, $sev:ident; )*) => {
        /// Every error code Zaun can report. See ERROR_HANDLING §3 for meanings and
        /// primary actions; `catalog::describe` produces the user-facing text.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum ErrorCode {
            $( $(#[$meta])* $variant, )*
        }

        impl ErrorCode {
            /// Every code, in catalogue order.
            pub const ALL: &'static [ErrorCode] = &[ $( ErrorCode::$variant, )* ];

            /// The stable string form, e.g. `ZN-HOST-001`.
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
    // ---- Host — ZN-HOST ------------------------------------------------------------------
    /// bubblewrap not found.
    HostBwrapNotFound = "ZN-HOST-001", Error;
    /// bubblewrap lacks a required option.
    HostBwrapMissingOption = "ZN-HOST-002", Error;
    /// bubblewrap is setuid-only and user namespaces are unavailable.
    HostBwrapSetuidOnly = "ZN-HOST-003", Error;
    /// Unprivileged user namespaces unavailable.
    HostUsernsUnavailable = "ZN-HOST-004", Error;
    /// Nested user namespaces unavailable.
    HostNestedUsernsUnavailable = "ZN-HOST-005", Error;
    /// `zaun-init` missing or not executable.
    HostInitMissing = "ZN-HOST-006", Error;
    /// `umu-run` not found.
    HostUmuNotFound = "ZN-HOST-010", Error;
    /// `umu-run` version too old.
    HostUmuTooOld = "ZN-HOST-011", Error;
    /// `umu-run` installation cannot be made visible safely.
    HostUmuClosureRejected = "ZN-HOST-012", Error;
    /// No display server socket available.
    HostNoDisplay = "ZN-HOST-020", Error;
    /// No GPU render node accessible.
    HostNoGpu = "ZN-HOST-021", Warning;
    /// Running as root is not supported.
    HostRunningAsRoot = "ZN-HOST-030", Error;
    /// musl-based system not supported by umu.
    HostMuslUnsupported = "ZN-HOST-031", Error;

    // ---- Sandbox — ZN-SBX ----------------------------------------------------------------
    /// Sandbox could not be set up (unclassified bwrap error).
    SbxSetupFailed = "ZN-SBX-001", Error;
    /// A permitted folder or file is missing.
    SbxGrantMissing = "ZN-SBX-010", Error;
    /// Internal planning error: mount on a symlink.
    SbxMountOnSymlink = "ZN-SBX-011", Error;
    /// Protected game folder (overlay) not supported here.
    SbxOverlayUnsupported = "ZN-SBX-012", Error;
    /// Init shim not runnable inside the sandbox.
    SbxInitNotRunnable = "ZN-SBX-013", Error;
    /// No way to show a window with the current display permissions.
    SbxNoDisplayAccess = "ZN-SBX-020", Error;
    /// Internal planning error: runtime layout.
    SbxRuntimeLayout = "ZN-SBX-030", Error;
    /// This game's permissions could not be read.
    SbxPolicyUnreadable = "ZN-SBX-040", Error;
    /// A permission is not allowed (deny-list).
    SbxDenied = "ZN-SBX-050", Error;
    /// A permission is unusually broad.
    SbxBroadGrant = "ZN-SBX-051", Warning;
    /// A granted folder now points somewhere else.
    SbxGrantTargetChanged = "ZN-SBX-052", Warning;
    /// Plan invariant violated (bug); fatal for the launch only.
    SbxPlanInvariant = "ZN-SBX-900", Fatal;

    // ---- Runtime — ZN-RT -----------------------------------------------------------------
    /// Compatibility layer not installed and no internet.
    RtNotInstalledOffline = "ZN-RT-010", Error;
    /// Compatibility layer could not be resolved.
    RtUnresolvable = "ZN-RT-011", Error;
    /// Compatibility layer folder is not valid.
    RtInvalidFolder = "ZN-RT-012", Error;
    /// The layer needs a runtime this system setup does not support.
    RtUnsupportedRuntime = "ZN-RT-013", Error;
    /// Download failed verification.
    RtVerificationFailed = "ZN-RT-014", Error;
    /// The selected layer is no longer installed.
    RtNoLongerInstalled = "ZN-RT-015", Error;
    /// Download interrupted.
    RtDownloadInterrupted = "ZN-RT-016", Error;
    /// A compatibility layer or runtime must be installed and this build cannot install it
    /// (provisioning arrives in M7).
    RtNotInstalled = "ZN-RT-017", Error;
    /// Shared runtime was modified and has been removed.
    RtSharedTampered = "ZN-RT-020", Warning;
    /// Runtime files damaged (no game suspected).
    RtDamaged = "ZN-RT-021", Warning;
    /// Not enough free space for a private runtime copy.
    RtNoSpaceForCopy = "ZN-RT-030", Error;
    /// Private Windows storage could not be prepared.
    RtPrefixPrepareFailed = "ZN-RT-040", Error;
    /// Private Windows storage appears damaged.
    RtPrefixDamaged = "ZN-RT-041", Error;

    // ---- umu and container — ZN-UMU ------------------------------------------------------
    /// The compatibility launcher stopped with an internal error.
    UmuInternalError = "ZN-UMU-001", Error;
    /// The program did not start (exited at once with an error).
    UmuProgramDidNotStart = "ZN-UMU-002", Error;
    /// The compatibility container could not start.
    UmuContainerFailed = "ZN-UMU-003", Error;
    /// A game fix needs the internet.
    UmuFixNeedsNetwork = "ZN-UMU-004", Warning;

    // ---- Session — ZN-SES ----------------------------------------------------------------
    /// Background process for the game did not start.
    SesSupervisorNotStarted = "ZN-SES-001", Error;
    /// Launch plan not understood (version mismatch after an update).
    SesPlanVersionMismatch = "ZN-SES-002", Error;
    /// Already running.
    SesAlreadyRunning = "ZN-SES-010", Info;
    /// The game did not stop cleanly; forced.
    SesForcedStop = "ZN-SES-020", Warning;
    /// Not permitted to signal the session.
    SesSignalDenied = "ZN-SES-021", Error;
    /// A previous session was interrupted and has been closed.
    SesInterrupted = "ZN-SES-030", Warning;
    /// No such session (anymore).
    SesSessionNotFound = "ZN-SES-040", Warning;

    // ---- Game — ZN-GAME ------------------------------------------------------------------
    /// The game closed right after starting.
    GameClosedEarly = "ZN-GAME-001", Warning;
    /// The game crashed.
    GameCrashed = "ZN-GAME-002", Warning;
    /// The game was stopped by the system (likely out of memory).
    GameKilledBySystem = "ZN-GAME-003", Warning;

    // ---- Library and identity — ZN-LIB, ZN-ID --------------------------------------------
    /// Program file not found.
    LibExeNotFound = "ZN-LIB-001", Error;
    /// The drive this game is on is not connected.
    LibDriveDisconnected = "ZN-LIB-002", Error;
    /// Program file cannot be read (permissions).
    LibExeUnreadable = "ZN-LIB-003", Error;
    /// This program is already in the library.
    LibAlreadyInLibrary = "ZN-LIB-010", Info;
    /// Another action for this game is still in progress.
    LibBusy = "ZN-LIB-020", Info;
    /// Name, tag or note not valid.
    LibInvalidField = "ZN-LIB-030", Error;
    /// No such game (anymore).
    LibGameNotFound = "ZN-LIB-040", Warning;
    /// No such tag (anymore).
    LibTagNotFound = "ZN-LIB-041", Warning;
    /// Not a Windows program Zaun can run.
    IdNotWindowsProgram = "ZN-ID-001", Error;
    /// The program's information could not be read; using the file name.
    IdPeUnreadable = "ZN-ID-002", Warning;

    // ---- Files — ZN-FS -------------------------------------------------------------------
    /// Disk full.
    FsDiskFull = "ZN-FS-001", Error;
    /// Low disk space before starting.
    FsLowSpace = "ZN-FS-002", Warning;
    /// Could not write (permission denied).
    FsWriteDenied = "ZN-FS-003", Error;
    /// Could not delete some files; will retry at next start.
    FsDeleteIncomplete = "ZN-FS-004", Error;
    /// Cannot read the file (permission).
    FsReadDenied = "ZN-FS-010", Error;
    /// The location did not respond in time.
    FsTimeout = "ZN-FS-011", Error;

    // ---- Database and configuration — ZN-DB, ZN-CFG --------------------------------------
    /// Library file damaged.
    DbCorrupt = "ZN-DB-001", Fatal;
    /// Library busy; change not saved.
    DbBusy = "ZN-DB-002", Error;
    /// Library created by a newer Zaun.
    DbNewerVersion = "ZN-DB-010", Fatal;
    /// Close running games before updating the library.
    DbSessionsRunning = "ZN-DB-011", Error;
    /// Could not back up before updating; update not attempted.
    DbBackupFailed = "ZN-DB-012", Fatal;
    /// Library update failed; previous version restored (recovery mode).
    DbMigrationFailed = "ZN-DB-013", Fatal;
    /// A setting was not valid and was not changed.
    CfgInvalidValue = "ZN-CFG-001", Warning;
    /// Settings file could not be read; defaults in use.
    CfgUnreadable = "ZN-CFG-002", Warning;
    /// Settings file is from a newer version; using defaults without changing it.
    CfgNewerVersion = "ZN-CFG-003", Warning;

    // ---- Artwork — ZN-ART ----------------------------------------------------------------
    /// Image format not supported.
    ArtUnsupportedFormat = "ZN-ART-001", Error;
    /// Image too large.
    ArtTooLarge = "ZN-ART-002", Error;
    /// Image could not be read.
    ArtUnreadable = "ZN-ART-003", Error;
    /// Steam has no official artwork for this game (or its app ID is unknown).
    ArtNotFound = "ZN-ART-004", Warning;
    /// Steam could not be reached for artwork or a search.
    ArtNoConnection = "ZN-ART-005", Warning;
    /// The program has no icon to use.
    ArtNoIcon = "ZN-ART-006", Warning;

    // ---- Steam games — ZN-STM (ADR-0031) -------------------------------------------------
    /// No Steam installation was found.
    StmNotFound = "ZN-STM-001", Warning;
    /// Steam no longer lists this game (it was uninstalled, or its drive is not connected).
    StmGameGone = "ZN-STM-002", Warning;
    /// Steam could not be asked (its program is missing or did not start).
    StmClientUnavailable = "ZN-STM-003", Error;
    /// The game is running, so nothing can run in its Windows storage now.
    StmGameRunning = "ZN-STM-004", Warning;
    /// The game has no Windows storage (not run through Proton, or not played yet).
    StmNoPrefix = "ZN-STM-005", Warning;
    /// The compatibility layer Steam uses for the game cannot be used here.
    StmLayerUnavailable = "ZN-STM-006", Error;
    /// The action does not apply to a game Steam runs.
    StmNotForSteam = "ZN-STM-007", Info;
    /// Steam is open, so its settings file cannot be changed now (Steam rewrites it when it
    /// closes).
    StmSteamOpen = "ZN-STM-008", Warning;
    /// Steam's settings file for the account could not be found, read safely or written.
    StmSettingsUnwritable = "ZN-STM-009", Error;

    // ---- Chat — ZN-CHT (ADR-0033) ------------------------------------------------------
    /// This build of Zaun has no chat engine (the Chat helper or its library is
    /// missing).
    ChtUnavailable = "ZN-CHT-001", Warning;
    /// The chat engine stopped again and again, so Chat was stopped.
    ChtEngineFailing = "ZN-CHT-002", Error;
    /// The encrypted chat database could not be opened.
    ChtDatabase = "ZN-CHT-003", Error;
    /// No relay could be reached (no connection, or the relays are down).
    ChtNoConnection = "ZN-CHT-004", Warning;
    /// The text is not a SimpleX invitation or address link.
    ChtBadLink = "ZN-CHT-005", Warning;
    /// The chat engine refused a request.
    ChtRequestFailed = "ZN-CHT-006", Warning;
    /// A file from a chat could not be saved where the user chose.
    ChtSaveFailed = "ZN-CHT-007", Warning;
    /// The file chosen as a profile picture is not a picture Zaun can read.
    ChtNotAPicture = "ZN-CHT-008", Warning;
    /// The file chosen as a profile picture is too large to read (over 20 MiB, or more than
    /// 16 384 pixels a side).
    ChtPictureTooLarge = "ZN-CHT-009", Warning;
    /// A call could not use a microphone (none, or the sound server refused it; ADR-0035).
    ChtNoMicrophone = "ZN-CHT-010", Warning;
    /// A call's sound could not start or stopped (speakers, the voice codec, the network
    /// socket).
    ChtMediaFailed = "ZN-CHT-011", Warning;
    /// A call was answered but its sound never got through (no path between the two
    /// computers, or the call relay could not be reached).
    ChtCallFailed = "ZN-CHT-012", Warning;
    /// The in-game overlay cannot be shown now: Zaun or a game is not running, or Chat
    /// or the overlay is off (ADR-0037).
    ChtOverlayUnavailable = "ZN-CHT-013", Warning;
    /// Sharing a game: the user closed the portal's window picker, or the desktop refused
    /// (ADR-0036).
    ChtShareRefused = "ZN-CHT-014", Warning;
    /// Sharing asked for while no game runs (ADR-0036).
    ChtNoGameRunning = "ZN-CHT-015", Warning;
    /// The game's window could not be captured: no ScreenCast portal, no window capture on
    /// this desktop, PipeWire failed, or this build cannot capture (ADR-0036).
    ChtCaptureFailed = "ZN-CHT-016", Warning;
    /// A stream could not start or was cut off: no connection to the friend's app, the
    /// engine refused it, a group, or a call with the same friend (ADR-0036).
    ChtStreamFailed = "ZN-CHT-017", Warning;

    // ---- Integration and internal — ZN-DSK, ZN-INT ---------------------------------------
    /// Could not change the default application.
    DskDefaultHandlerFailed = "ZN-DSK-001", Error;
    /// Could not create the shortcut.
    DskShortcutFailed = "ZN-DSK-002", Error;
    /// Another Zaun instance did not respond.
    DskInstanceUnresponsive = "ZN-DSK-003", Warning;
    /// An internal task failed unexpectedly.
    IntTaskFailed = "ZN-INT-001", Error;
    /// Zaun has to close (unrecoverable internal error).
    IntFatal = "ZN-INT-002", Fatal;
    /// This feature is not available yet in this build (placeholder for a later milestone;
    /// the context carries the tracking task ID).
    IntNotAvailable = "ZN-INT-003", Info;
}

impl ErrorCode {
    /// The area part, e.g. `HOST` for `ZN-HOST-001`.
    pub fn area(self) -> &'static str {
        let s = self.as_str();
        let rest = &s[3..];
        match rest.find('-') {
            Some(i) => &rest[..i],
            None => rest,
        }
    }

    /// The numeric part, e.g. `1` for `ZN-HOST-001`.
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
            assert_eq!(parts[0], "ZN", "{s}");
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
        assert!("ZN-NOPE-001".parse::<ErrorCode>().is_err());
        assert_eq!(ErrorCode::SbxPlanInvariant.number(), 900);
        assert_eq!(ErrorCode::LibInvalidField.area(), "LIB");
    }
}
