//! `PolicyIssue` and its stable codes.

use std::borrow::Cow;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::ErrorCode;
use crate::model::GameFlags;
use crate::sandbox::plan::codec;

/// How serious an issue is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    /// Shown, never blocks.
    Info,
    /// Launch allowed after acknowledgement.
    Warning,
    /// The policy cannot be used as is.
    Error,
    /// A planner bug caught by the plan invariants (`EP-SBX-900`).
    Fatal,
}

macro_rules! issue_codes {
    ($( $(#[$m:meta])* $name:ident = $text:literal => $ep:ident ),* $(,)?) => {
        /// Stable issue code (`SEC-*`).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum IssueCode {
            $( $(#[$m])* #[serde(rename = $text)] $name, )*
        }

        impl IssueCode {
            /// Every code.
            pub const ALL: &'static [IssueCode] = &[$(IssueCode::$name),*];

            /// Stable text, e.g. `SEC-PATH-010`.
            pub const fn as_str(self) -> &'static str {
                match self { $(IssueCode::$name => $text,)* }
            }

            /// The error code a launch fails with because of this issue.
            pub const fn error_code(self) -> ErrorCode {
                match self { $(IssueCode::$name => ErrorCode::$ep,)* }
            }
        }
    };
}

issue_codes! {
    /// Path is not absolute.
    PathNotAbsolute = "SEC-PATH-001" => SbxDenied,
    /// Path does not exist or has the wrong type.
    PathMissing = "SEC-PATH-002" => SbxGrantMissing,
    /// Path contains a NUL byte or is not lexically normalised.
    PathInvalid = "SEC-PATH-003" => SbxDenied,
    /// The canonical target of a grant changed since it was granted.
    PathTargetChanged = "SEC-PATH-004" => SbxGrantTargetChanged,
    /// Path is on a pseudo-filesystem.
    PathPseudoFs = "SEC-PATH-005" => SbxDenied,
    /// Path stands in a denied relation to a deny-list entry.
    PathDenied = "SEC-PATH-010" => SbxDenied,
    /// A deny-list entry is exposed through a Developer Mode exception (shown in red).
    PathDenyBypassed = "SEC-PATH-011" => SbxDenied,
    /// A grant contains or equals the game folder (it would hide the game folder mount).
    PathCoversGameDir = "SEC-PATH-012" => SbxDenied,
    /// Two grants or exceptions for the same path.
    PathDuplicate = "SEC-PATH-013" => SbxDenied,
    /// Read-write grant on a directory with very many entries.
    BroadGrant = "SEC-PATH-020" => SbxBroadGrant,
    /// Read-write grant on a mount root.
    MountRootGrant = "SEC-PATH-021" => SbxBroadGrant,
    /// Game folder is an XDG user directory.
    GameDirUserDir = "SEC-PATH-022" => SbxBroadGrant,
    /// Game folder contains another game's folder.
    GameDirContainsGame = "SEC-PATH-023" => SbxBroadGrant,
    /// The executable is not inside the game folder.
    ExeOutsideRoot = "SEC-PATH-030" => SbxDenied,
    /// `FileOnly` iff there is no game folder (CONFIGURATION §3 rule 3).
    GameDirKindMismatch = "SEC-PATH-031" => SbxDenied,
    /// External save directory conflicts with the game folder (CONFIGURATION §3 rule 7).
    SaveDirConflict = "SEC-PATH-032" => SbxDenied,
    /// Custom working directory is not inside a mounted location.
    WorkingDirOutside = "SEC-PATH-040" => SbxDenied,
    /// `Protected` game folder without overlay support.
    OverlayUnsupported = "SEC-OVERLAY-001" => SbxOverlayUnsupported,
    /// `Protected` game folder on a filesystem that cannot be an overlay lower layer.
    OverlayLowerFs = "SEC-OVERLAY-002" => SbxOverlayUnsupported,
    /// Network on together with a read-write grant outside the game folder.
    NetworkWithWritableGrant = "SEC-NET-001" => SbxBroadGrant,
    /// `AllInput` requested but keyboards are readable; downgraded to `Detected`.
    AllInputUnsafe = "SEC-INPUT-001" => SbxDenied,
    /// X11 session: display isolation is weak.
    X11Session = "SEC-DISPLAY-001" => SbxNoDisplayAccess,
    /// Neither display socket can be provided.
    NoDisplay = "SEC-DISPLAY-002" => SbxNoDisplayAccess,
    /// The chosen window system's socket is denied (CONFIGURATION §3 rule 5).
    WindowSystemDenied = "SEC-DISPLAY-003" => SbxNoDisplayAccess,
    /// Policy document is newer than this build.
    SchemaTooNew = "SEC-POLICY-001" => SbxPolicyUnreadable,
    /// A reserved enum value (`Gamescope`, `Filtered`).
    ReservedValue = "SEC-POLICY-002" => SbxPolicyUnreadable,
    /// Structural policy validation failed.
    PolicyMalformed = "SEC-POLICY-003" => SbxPolicyUnreadable,
    /// The session context is inconsistent (missing game or runtime for the kind).
    ContextInvalid = "SEC-POLICY-004" => SbxPlanInvariant,
    /// A user variable has an invalid name or value.
    EnvInvalid = "SEC-ENV-001" => SbxDenied,
    /// A user variable uses a reserved name.
    EnvReserved = "SEC-ENV-002" => SbxDenied,
    /// A dangerous variable without Developer Mode.
    EnvDangerous = "SEC-ENV-003" => SbxDenied,
    /// An exception needs Developer Mode.
    ExceptionNeedsDevMode = "SEC-EXC-001" => SbxDenied,
    /// `ExtraDevice` is not an allowed character device.
    ExceptionDevice = "SEC-EXC-002" => SbxDenied,
    /// Exception reason shorter than 10 characters.
    ExceptionReason = "SEC-EXC-003" => SbxDenied,
    /// The runtime store layout could not be read or is incomplete.
    RuntimeLayout = "SEC-RT-001" => SbxRuntimeLayout,
    /// Invariant 1: required namespaces / network flag.
    InvNamespaces = "SEC-PLAN-001" => SbxPlanInvariant,
    /// Invariant 2: lifecycle flags.
    InvLifecycle = "SEC-PLAN-002" => SbxPlanInvariant,
    /// Invariant 3: environment.
    InvEnv = "SEC-PLAN-003" => SbxPlanInvariant,
    /// Invariant 4: deny-listed source.
    InvDenied = "SEC-PLAN-004" => SbxPlanInvariant,
    /// Invariant 5: writable runtime store.
    InvRuntimeWritable = "SEC-PLAN-005" => SbxPlanInvariant,
    /// Invariant 6: writable bind into another game.
    InvOtherGame = "SEC-PLAN-006" => SbxPlanInvariant,
    /// Invariant 7: Proton writable.
    InvProtonWritable = "SEC-PLAN-007" => SbxPlanInvariant,
    /// Invariant 8: unexpected socket.
    InvSocket = "SEC-PLAN-008" => SbxPlanInvariant,
    /// Invariant 9: destination rules.
    InvDestination = "SEC-PLAN-009" => SbxPlanInvariant,
    /// Invariant 10: command shape.
    InvCommand = "SEC-PLAN-010" => SbxPlanInvariant,
    /// Planner misuse (builder order).
    InvBuilder = "SEC-PLAN-011" => SbxPlanInvariant,
}

/// One finding of validation or planning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyIssue {
    /// Severity.
    pub severity: IssueSeverity,
    /// Stable code.
    pub code: IssueCode,
    /// The path concerned, if any.
    #[serde(with = "codec::opt_path")]
    pub path: Option<PathBuf>,
    /// Message key with English fallback text after `|`, e.g.
    /// `sec.path.denied|"~/.ssh" holds credentials`.
    pub message: Cow<'static, str>,
    /// The `GameFlags` bit that acknowledges this warning, where specified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ack: Option<u32>,
}

impl PolicyIssue {
    /// A new issue.
    pub fn new(
        severity: IssueSeverity,
        code: IssueCode,
        path: Option<PathBuf>,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            severity,
            code,
            path,
            message: message.into(),
            ack: None,
        }
    }

    /// An `Error` issue.
    pub fn error(
        code: IssueCode,
        path: Option<PathBuf>,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::new(IssueSeverity::Error, code, path, message)
    }

    /// A `Warning` issue.
    pub fn warning(
        code: IssueCode,
        path: Option<PathBuf>,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::new(IssueSeverity::Warning, code, path, message)
    }

    /// A `Fatal` issue (invariant violation).
    pub fn fatal(
        code: IssueCode,
        path: Option<PathBuf>,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::new(IssueSeverity::Fatal, code, path, message)
    }

    /// Attach the acknowledgement flag.
    #[must_use]
    pub fn acknowledged_by(mut self, flag: GameFlags) -> Self {
        self.ack = Some(flag.bits());
        self
    }

    /// The acknowledgement flag.
    pub fn ack_flag(&self) -> Option<GameFlags> {
        self.ack.map(GameFlags::from_bits_retain)
    }

    /// Whether the issue prevents launching.
    pub fn is_blocking(&self) -> bool {
        self.severity >= IssueSeverity::Error
    }
}

/// Whether any issue blocks.
pub fn any_blocking(issues: &[PolicyIssue]) -> bool {
    issues.iter().any(PolicyIssue::is_blocking)
}
