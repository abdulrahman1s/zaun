//! Dialogs (SCREEN_SPECS_2 §16.1) and prompts (LOGIC_SPECS §4).

use zaun_core::model::BasePreset;

texts! {
    /// Button.
    CANCEL = "dialogs.cancel";
    /// Button.
    CLOSE = "dialogs.close";
    /// Button.
    REMOVE = "dialogs.remove";
    /// Button.
    RESET = "dialogs.reset";
    /// Button.
    KEEP_EDITING = "dialogs.keep_editing";
    /// Button.
    DISCARD = "dialogs.discard";
    /// Button.
    ALLOW = "dialogs.allow";
    /// Button.
    REMOVE_LAYER = "dialogs.remove_layer";
    /// Button.
    CLEAR = "dialogs.clear";
    /// Button.
    TURN_ON = "dialogs.turn_on";
    /// Button.
    SET_UP_NEW = "dialogs.set_up_new";
    /// Button.
    ITS_AN_UPDATE = "dialogs.its_an_update";
    /// Button.
    RUN_WITH_SETTINGS = "dialogs.run_with_settings";
    /// Button.
    USE_SAME = "dialogs.use_same";
    /// Button.
    ALLOW_NEW_LOCATION = "dialogs.allow_new_location";
    /// Button.
    USE_DEFAULT = "dialogs.use_default";
    /// Button.
    NOT_NOW = "dialogs.not_now";
    /// Button.
    TURN_OFF_FIXES = "dialogs.turn_off_fixes";
    /// Button.
    ALLOW_ONCE = "dialogs.allow_once";
    /// Button.
    KEEP_AS_INSTALLER = "dialogs.keep_as_installer";
    /// Button.
    REMOVE_ENTRY = "dialogs.remove_entry";
    /// Button.
    USE_PROGRAM = "dialogs.use_program";
    /// Button.
    SHARE_ANYWAY = "dialogs.share_anyway";
    /// Button.
    STOP_AND_REMOVE = "dialogs.stop_and_remove";
    /// Remove body.
    REMOVE_BODY = "dialogs.remove_body";
    /// Option.
    REMOVE_STORAGE = "dialogs.remove_storage";
    /// Option hint.
    REMOVE_STORAGE_HINT = "dialogs.remove_storage_hint";
    /// Option.
    KEEP_SAVES = "dialogs.keep_saves";
    /// Option.
    REMOVE_SHORTCUTS = "dialogs.remove_shortcuts";
    /// Option.
    KEEP_HISTORY = "dialogs.keep_history";
    /// Discard.
    DISCARD_TITLE = "dialogs.discard_title";
    /// Discard.
    DISCARD_BODY = "dialogs.discard_body";
    /// Clear statistics.
    CLEAR_TITLE = "dialogs.clear_title";
    /// Developer Mode.
    DEV_TITLE = "dialogs.dev_title";
    /// Developer Mode.
    DEV_BODY = "dialogs.dev_body";
    /// Developer Mode.
    DEV_UNSAFE = "dialogs.dev_unsafe";
    /// Exe changed.
    EXE_CHANGED_BODY = "dialogs.exe_changed_body";
    /// Runtime missing.
    RUNTIME_MISSING_BODY = "dialogs.runtime_missing_body";
    /// Fix needs network.
    FIX_NETWORK_TITLE = "dialogs.fix_network_title";
    /// Tool needs network.
    TOOL_NETWORK_TITLE = "dialogs.tool_network_title";
    /// Tainted prefix.
    TAINTED = "dialogs.tainted";
    /// Installer finished.
    INSTALLER_TITLE = "dialogs.installer_title";
    /// Grant moved.
    GRANT_MOVED_TITLE = "dialogs.grant_moved_title";
    /// Broad grant.
    BROAD_REMINDER = "dialogs.broad_reminder";
    /// Removing a running game.
    RUNNING_REMOVE_BODY = "dialogs.running_remove_body";
    /// Remember choice.
    REMEMBER = "dialogs.remember";
    /// Button: removing several games.
    REMOVE_MANY = "dialogs.remove_many";
    /// Remove body, several games.
    REMOVE_BODY_MANY = "dialogs.remove_body_many";
    /// Clear all statistics: the text.
    CLEAR_ALL_TEXT = "dialogs.clear_all_text";
    /// Button: clear all statistics.
    CLEAR_ALL = "dialogs.clear_all";
    /// Reset private Windows storage: the text.
    RESET_TEXT = "dialogs.reset_text";
    /// The report dialog's text.
    REPORT_TEXT = "dialogs.report_text";
}

/// "Remove Lantern Keep?"
pub fn remove_title(name: &str) -> String {
    rust_i18n::t!("dialogs.remove_title", name = name).into_owned()
}
/// "Remove 3 games?"
pub fn remove_many_title(n: usize) -> String {
    rust_i18n::t!("dialogs.remove_many_title", n = n).into_owned()
}
/// "Reset private Windows storage for Hollow Knight?"
pub fn reset_title(name: &str) -> String {
    rust_i18n::t!("dialogs.reset_title", name = name).into_owned()
}
/// "Give Hollow Knight internet access?"
pub fn widen_title(name: &str, p: BasePreset) -> String {
    match p {
        BasePreset::Standard => {
            rust_i18n::t!("dialogs.widen_title_standard", name = name).into_owned()
        }
        _ => rust_i18n::t!("dialogs.widen_title_online", name = name).into_owned(),
    }
}
/// "Give 3 games internet access?"
pub fn widen_many_title(n: usize, p: BasePreset) -> String {
    match p {
        BasePreset::Standard => {
            crate::i18n::plural::counted("dialogs.widen_many_standard", n as u64)
        }
        _ => crate::i18n::plural::counted("dialogs.widen_many_online", n as u64),
    }
}
/// "1 running game is skipped."
pub fn skipped_running(n: usize) -> String {
    crate::i18n::plural::counted("dialogs.skipped_running", n as u64)
}
/// "Remove GE-Proton 10-25?"
pub fn remove_layer_title(label: &str) -> String {
    rust_i18n::t!("dialogs.remove_layer_title", label = label).into_owned()
}
/// "Frees 1.4 GB."
pub fn frees(bytes: u64) -> String {
    rust_i18n::t!("dialogs.frees", size = super::fmt::bytes(bytes)).into_owned()
}
/// "No games use it." / "3 games use it; they switch to the default."
pub fn layer_users(n: u32) -> String {
    match n {
        0 => crate::i18n::text("dialogs.layer_users_none").to_owned(),
        n => crate::i18n::plural::counted("dialogs.layer_users", u64::from(n)),
    }
}
/// "Every recorded session of Lantern Keep is deleted…"
pub fn clear_game_text(name: &str) -> String {
    rust_i18n::t!("dialogs.clear_game_text", name = name).into_owned()
}
/// "It will be able to use the internet and your local network."
pub fn widen_text(p: BasePreset, skipped: usize) -> String {
    let can = match p {
        BasePreset::Offline => String::new(),
        BasePreset::Online => crate::i18n::text("dialogs.widen_text.online").to_owned(),
        BasePreset::Standard => crate::i18n::text("dialogs.widen_text.standard").to_owned(),
    };
    match skipped {
        0 => can,
        n => format!("{can} {}", skipped_running(n)).trim().to_owned(),
    }
}
/// "Clear play history for Hollow Knight?"
pub fn clear_game_title(name: &str) -> String {
    rust_i18n::t!("dialogs.clear_game_title", name = name).into_owned()
}
/// "Hollow Knight looks different"
pub fn exe_changed_title(name: &str) -> String {
    rust_i18n::t!("dialogs.exe_changed_title", name = name).into_owned()
}
/// "Was 32 MB, now 33 MB."
pub fn exe_changed_sizes(old: u64, new: u64) -> String {
    rust_i18n::t!(
        "dialogs.exe_changed_sizes",
        old = super::fmt::bytes(old),
        new = super::fmt::bytes(new)
    )
    .into_owned()
}
/// "Run Config.exe with Hollow Knight's settings?"
pub fn inside_root_title(file: &str, name: &str) -> String {
    rust_i18n::t!("dialogs.inside_root_title", file = file, name = name).into_owned()
}
/// Body.
pub fn inside_root_body(name: &str) -> String {
    rust_i18n::t!("dialogs.inside_root_body", name = name).into_owned()
}
/// "This is a copy of Hollow Knight"
pub fn copy_title(name: &str) -> String {
    rust_i18n::t!("dialogs.copy_title", name = name).into_owned()
}
/// Body.
pub fn copy_body(file: &str, name: &str) -> String {
    rust_i18n::t!("dialogs.copy_body", file = file, name = name).into_owned()
}
/// Body.
pub fn grant_moved_body(name: &str, folder: &str, new: &str) -> String {
    rust_i18n::t!(
        "dialogs.grant_moved_body",
        name = name,
        folder = folder,
        new = new
    )
    .into_owned()
}
/// "The compatibility layer for Hollow Knight is missing"
pub fn runtime_missing_title(name: &str) -> String {
    rust_i18n::t!("dialogs.runtime_missing_title", name = name).into_owned()
}
/// Body.
pub fn fix_network_body(name: &str) -> String {
    rust_i18n::t!("dialogs.fix_network_body", name = name).into_owned()
}
/// Body.
pub fn tool_network_body(name: &str) -> String {
    rust_i18n::t!("dialogs.tool_network_body", name = name).into_owned()
}
/// Body.
pub fn installer_body(name: &str) -> String {
    rust_i18n::t!("dialogs.installer_body", name = name).into_owned()
}
/// "Celeste.exe · 32 MB".
pub fn program_choice(label: &str, size: u64) -> String {
    rust_i18n::t!(
        "dialogs.program_choice",
        label = label,
        size = super::fmt::bytes(size)
    )
    .into_owned()
}
/// "Share all of Documents?"
pub fn broad_title(folder: &str) -> String {
    rust_i18n::t!("dialogs.broad_title", folder = folder).into_owned()
}
/// The reason, from a catalogue key.
pub fn broad_reason(key: &str) -> String {
    match key {
        "mount_root" => crate::i18n::text("dialogs.broad_reason.mount_root").to_owned(),
        "user_dir" => crate::i18n::text("dialogs.broad_reason.user_dir").to_owned(),
        _ => crate::i18n::text("dialogs.broad_reason.many_files").to_owned(),
    }
}
/// "Hollow Knight is running"
pub fn running_remove_title(name: &str) -> String {
    rust_i18n::t!("dialogs.running_remove_title", name = name).into_owned()
}
/// "1 more problem".
pub fn more_problems(n: usize) -> String {
    crate::i18n::plural::counted("dialogs.more_problems", n as u64)
}
