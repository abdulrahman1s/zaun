//! Error presentation (SCREEN_SPECS_2 §16): recovery action labels and the copied details.

use easyplay_core::error::{RecoveryAction, UserError};
use easyplay_core::model::BasePreset;

/// The button label of a recovery action.
pub fn action(a: RecoveryAction) -> &'static str {
    use RecoveryAction as R;
    match a {
        R::Retry => "Try again",
        R::EditPermissions => "Edit permissions",
        R::ChooseGameFolder => "Choose a game folder",
        R::LocateExecutable => "Locate…",
        R::RemoveGame => "Remove game…",
        R::SwitchPreset(BasePreset::Offline) => "Switch to Offline",
        R::SwitchPreset(BasePreset::Online) => "Switch to Online",
        R::SwitchPreset(BasePreset::Standard) => "Try Standard protection",
        R::AllowGameFolderWrites => "Allow changes in the game folder",
        R::ChangeRuntime => "Try a different compatibility layer",
        R::UseDefaultRuntime => "Use the default layer",
        R::DownloadRuntime => "Download now",
        R::ResetPrefix => "Reset private Windows storage…",
        R::OpenDoctor => "Check my system",
        R::OpenSettings(_) => "Open settings",
        R::ViewLog => "View log",
        R::OpenSessionFolder => "Open session folder",
        R::CopyDetails => "Copy details",
        R::RestoreBackup => "Restore library…",
        R::OpenBackupsFolder => "Open backups folder",
        R::FreeSpaceInfo => "Show disk space",
        R::ShowRunningGame => "Show the game",
        R::CreateReport => "Create a report…",
        R::Quit => "Quit",
        R::Dismiss => "Close",
    }
}

/// The redacted text copied by *Copy details*: title, message, details and code.
pub fn copy_text(e: &UserError) -> String {
    let mut s = format!("{}\n{}\n", e.title, e.message);
    if let Some(d) = &e.details {
        s.push_str(d);
        s.push('\n');
    }
    s.push_str(e.code.as_str());
    s
}
