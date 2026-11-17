//! Error presentation (SCREEN_SPECS_2 §16): recovery action labels and the copied details.

use easyplay_core::error::{RecoveryAction, UserError};
use easyplay_core::model::BasePreset;

/// The button label of a recovery action.
pub fn action(a: RecoveryAction) -> &'static str {
    use RecoveryAction as R;
    match a {
        R::Retry => crate::i18n::text("errors.action.try_again"),
        R::EditPermissions => crate::i18n::text("errors.action.edit_permissions"),
        R::ChooseGameFolder => crate::i18n::text("errors.action.choose_a_game_folder"),
        R::LocateExecutable => crate::i18n::text("errors.action.locate"),
        R::RemoveGame => crate::i18n::text("errors.action.remove_game"),
        R::SwitchPreset(BasePreset::Offline) => {
            crate::i18n::text("errors.action.switch_to_offline")
        }
        R::SwitchPreset(BasePreset::Online) => crate::i18n::text("errors.action.switch_to_online"),
        R::SwitchPreset(BasePreset::Standard) => {
            crate::i18n::text("errors.action.try_standard_protection")
        }
        R::AllowGameFolderWrites => crate::i18n::text("errors.action.allow_changes_in_the"),
        R::ChangeRuntime => crate::i18n::text("errors.action.try_a_different_compatibility"),
        R::UseDefaultRuntime => crate::i18n::text("errors.action.use_the_default_layer"),
        R::DownloadRuntime => crate::i18n::text("errors.action.download_now"),
        R::ResetPrefix => crate::i18n::text("errors.action.reset_private_windows_storage"),
        R::OpenDoctor => crate::i18n::text("errors.action.check_my_system"),
        R::OpenSettings(_) => crate::i18n::text("errors.action.open_settings"),
        R::ViewLog => crate::i18n::text("errors.action.view_log"),
        R::OpenSessionFolder => crate::i18n::text("errors.action.open_session_folder"),
        R::CopyDetails => crate::i18n::text("errors.action.copy_details"),
        R::RestoreBackup => crate::i18n::text("errors.action.restore_library"),
        R::OpenBackupsFolder => crate::i18n::text("errors.action.open_backups_folder"),
        R::FreeSpaceInfo => crate::i18n::text("errors.action.show_disk_space"),
        R::ShowRunningGame => crate::i18n::text("errors.action.show_the_game"),
        R::CreateReport => crate::i18n::text("errors.action.create_a_report"),
        R::Quit => crate::i18n::text("errors.action.quit"),
        R::Dismiss => crate::i18n::text("errors.action.close"),
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
