//! What the UI receives: a fully described, user-facing error (ERROR_HANDLING §2).

use serde::{Deserialize, Serialize};

use super::ErrorCode;
use crate::ids::{GameId, SessionId};
use crate::model::BasePreset;

/// How serious a user-facing error is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational.
    Info,
    /// Something was recovered from or needs attention.
    Warning,
    /// An operation failed.
    Error,
    /// The application (or, for `EP-SBX-900`, the launch) cannot continue.
    Fatal,
}

/// A tab of the Settings screen, as a recovery target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsTab {
    /// General.
    General,
    /// Appearance.
    Appearance,
    /// Protection defaults.
    Protection,
    /// Compatibility layers.
    Runtimes,
    /// Desktop integration.
    Integration,
    /// Advanced.
    Advanced,
    /// About.
    About,
}

/// An action offered to recover from an error. Ordered lists; the first is primary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    /// Try the same operation again.
    Retry,
    /// Open the game's permissions.
    EditPermissions,
    /// Pick another game folder.
    ChooseGameFolder,
    /// Locate a moved program file.
    LocateExecutable,
    /// Remove the game from the library.
    RemoveGame,
    /// Switch the game to another preset.
    SwitchPreset(BasePreset),
    /// Allow the game to write to its folder.
    AllowGameFolderWrites,
    /// Choose another compatibility layer.
    ChangeRuntime,
    /// Use the default compatibility layer.
    UseDefaultRuntime,
    /// Download the compatibility layer.
    DownloadRuntime,
    /// Reset the game's private Windows storage.
    ResetPrefix,
    /// Open the Doctor screen.
    OpenDoctor,
    /// Open a Settings tab.
    OpenSettings(SettingsTab),
    /// Show the session log.
    ViewLog,
    /// Open the session folder.
    OpenSessionFolder,
    /// Copy the technical details.
    CopyDetails,
    /// Restore a library backup.
    RestoreBackup,
    /// Open the backups folder.
    OpenBackupsFolder,
    /// Explain disk space.
    FreeSpaceInfo,
    /// Show the running game.
    ShowRunningGame,
    /// Create a diagnostics report.
    CreateReport,
    /// Quit EasyPlay.
    Quit,
    /// Dismiss the message.
    Dismiss,
}

/// A user-facing error produced only by `catalog::describe`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserError {
    /// The stable code.
    pub code: ErrorCode,
    /// How serious it is.
    pub severity: Severity,
    /// What happened.
    pub title: String,
    /// What it means, likely cause. Never contains secrets or unredacted home paths.
    pub message: String,
    /// Ordered recovery actions; the first is primary. May be empty when nothing can be done
    /// (the message then says so).
    pub actions: Vec<RecoveryAction>,
    /// Redacted technical text, shown on demand.
    pub details: Option<String>,
    /// The game the error is about.
    pub game: Option<GameId>,
    /// The session the error is about.
    pub session: Option<SessionId>,
}

impl UserError {
    /// The primary action, if any.
    pub fn primary_action(&self) -> Option<RecoveryAction> {
        self.actions.first().copied()
    }
}
