//! Pure domain types shared by every layer and by the UI.
//!
//! Layer 1. Full definitions: DATA_MODEL. Types that are persisted derive
//! `Clone, Debug, PartialEq, Serialize, Deserialize`; JSON uses `snake_case` field names and
//! externally tagged `snake_case` enums. This module performs no I/O; validation that needs
//! the host lives in `sandbox::validate`.
//!
//! Dependencies: `ids`, `consts`, `clock` (for [`Timestamp`](crate::clock::Timestamp)) and
//! `error::ErrorCode` (for [`EndReason::FailedToStart`]).

mod artwork;
mod chat;
mod doc;
mod executable;
mod game;
mod launch_config;
mod policy;
mod prompt;
mod runtime;
mod session;
mod stats;
mod steam;
mod tag;

pub use artwork::{ArtOrigin, ArtSlot, ArtworkRef};
pub use chat::{
    ChatGroup, ChatMessage, ChatPreview, ChatProfile, ChatRef, ChatSnapshot, ChatStatus, Delivery,
    Friend, FriendRequest, Membership, MessageAuthor, MessageBody,
};
pub use doc::{DocError, UpgradeStep, parse_launch_config, parse_policy};
pub use executable::{
    EXE_LABEL_MAX_CHARS, ExeRole, Executable, Fingerprint, PeInfo, PeMachine, PeSubsystem, Sha256,
    from_hex32, to_hex,
};
pub use game::{
    Activity, ArtKey, Availability, EntryKind, Game, GameFlags, GameSummary, LaunchStateTag,
    SaveLocationKind, Store, SummaryFlags, sort_name_of, validate_name, validate_notes,
    validate_umu_id,
};
pub use launch_config::{
    CompatOptions, EnvVar, LaunchConfig, Renderer, SplitError, SyncMode, Tool, WindowSystem,
    WorkingDir, is_valid_locale, is_valid_winetricks_verb, join_args, split_args, validate_args,
};
pub use policy::{
    AudioAccess, BasePreset, ControllerAccess, DeviceAccess, DisplayAccess, DisplayIsolation,
    FolderAccess, FolderGrant, GameDirAccess, GrantOrigin, Hardening, IpcAccess, NetworkAccess,
    Permissions, PolicyException, Preset, RemovableAccess, RuntimeIsolation, SandboxPolicy,
    SaveDataLocation, SeccompMode, SessionBusAccess, SocketAccess, UserDir, is_valid_drive_letter,
};
pub use prompt::{ProgramCandidate, PromptAnswer, PromptKind, PromptRequest};
pub use runtime::{
    Channel, ResolvedRuntime, RuntimeBuild, RuntimeFamily, RuntimeIsolationMode, RuntimeOrigin,
    RuntimeRef, SlrVariant,
};
pub use session::{EndReason, Session, SessionContext, SessionKind, SessionState, SupervisorIdent};
pub use stats::{GameKey, GameStats, PlaytimeDay};
pub use steam::{
    GameOrigin, PlaytimeSource, ProtectionState, SteamCompat, SteamDownload, SteamFacts,
    SteamInstallKind, SteamRequest, SteamUpdate,
};
pub use tag::{Tag, TagColor};

use crate::error::{Error, ErrorCode};

/// What is wrong with a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Problem {
    /// Empty (after trimming, where text is trimmed).
    Empty,
    /// Longer than `max` (characters for text, bytes where the limit is in bytes).
    TooLong {
        /// The limit.
        max: usize,
    },
    /// More than `max` entries.
    TooMany {
        /// The limit.
        max: usize,
    },
    /// Outside the allowed range.
    OutOfRange,
    /// Contains characters that are not allowed.
    InvalidCharacters,
    /// Does not have the required format.
    InvalidFormat,
    /// A path that must be absolute is not.
    NotAbsolute,
    /// Contains a NUL character.
    ContainsNul,
    /// A value that must be unique appears twice.
    Duplicate,
    /// A value reserved for a later version.
    Reserved,
    /// A document version this build cannot read.
    UnsupportedVersion {
        /// The version found.
        found: u32,
        /// The newest supported version.
        max: u32,
    },
    /// A game must have exactly one primary executable.
    ExactlyOnePrimary {
        /// How many were found.
        found: usize,
    },
}

/// A field limit violated (the "typed error" of STORE-001).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{field}: {problem:?}")]
pub struct ValidationError {
    /// Dotted field path, e.g. `launch.args`.
    pub field: &'static str,
    /// The problem.
    pub problem: Problem,
}

impl ValidationError {
    /// A new validation error.
    pub const fn new(field: &'static str, problem: Problem) -> Self {
        Self { field, problem }
    }
}

impl From<ValidationError> for Error {
    fn from(e: ValidationError) -> Self {
        let code = match e.problem {
            Problem::UnsupportedVersion { .. } if e.field.starts_with("policy.") => {
                ErrorCode::SbxPolicyUnreadable
            }
            _ => ErrorCode::LibInvalidField,
        };
        Error::new(code)
            .with("field", e.field)
            .with("problem", format!("{:?}", e.problem))
    }
}

#[cfg(test)]
mod tests;
