//! First-time setup sheet (SCREEN_SPECS §6) and the add-game sheet (§5).

use std::path::Path;

use zaun_core::model::BasePreset;

texts! {
    /// Sheet title.
    TITLE = "setup.title";
    /// Title after Play.
    STARTING_TITLE = "setup.starting_title";
    /// Field.
    NAME = "setup.name";
    /// Validation.
    NAME_REQUIRED = "setup.name_required";
    /// Label.
    GAME_FOLDER = "setup.game_folder";
    /// Option.
    FILE_ONLY = "setup.file_only";
    /// Option.
    CHOOSE_FOLDER = "setup.choose_folder";
    /// Loose file explanation.
    LOOSE_FILE = "setup.loose_file";
    /// Honesty line.
    HONESTY = "setup.honesty";
    /// Link.
    LEARN_MORE = "setup.learn_more";
    /// Popover.
    HONESTY_MORE = "setup.honesty_more";
    /// Hint.
    INSTALLER_HINT = "setup.installer_hint";
    /// Hint.
    ONLINE_HINT = "setup.online_hint";
    /// Acknowledgement.
    I_UNDERSTAND = "setup.i_understand";
    /// Disclosure.
    MORE_OPTIONS = "setup.more_options";
    /// Section.
    FILES = "setup.files";
    /// Section.
    LAUNCH = "setup.launch";
    /// Section.
    TYPE = "setup.type";
    /// Option.
    KIND_GAME = "setup.kind_game";
    /// Option.
    KIND_INSTALLER = "setup.kind_installer";
    /// Option.
    KIND_TOOL = "setup.kind_tool";
    /// Button.
    ADD_TO_LIBRARY = "setup.add_to_library";
    /// Button.
    USE_FOR_REST = "setup.use_for_rest";
    /// Button.
    RUN_IN_BACKGROUND = "setup.run_in_background";
    /// Disabled reason.
    FIX_TO_CONTINUE = "setup.fix_to_continue";
}

/// "1 of 3".
pub fn queue(i: u32, n: u32) -> String {
    rust_i18n::t!("setup.queue", i = i, n = n).into_owned()
}

/// "Games ▸ Hollow Knight".
pub fn breadcrumb(p: &Path, home: &Path) -> String {
    let rel = p.strip_prefix(home).unwrap_or(p);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|c| c != "/")
        .collect::<Vec<_>>()
        .join(crate::i18n::text("setup.breadcrumb_separator"))
}

/// "Online: can use the internet and your local network".
pub fn announce_preset(b: BasePreset, first: Option<&str>) -> String {
    let name = super::shield::preset_name(b.into());
    match first {
        // Lower-casing is a no-op in scripts without case.
        Some(f) => rust_i18n::t!(
            "setup.announce_preset",
            name = name,
            first = f.to_lowercase()
        )
        .into_owned(),
        None => name.to_owned(),
    }
}

// ---- Add game ----------------------------------------------------------------------------

texts! {
    /// Sheet.
    ADD_TITLE = "setup.add_title";
    /// Scanning.
    LOOKING = "setup.looking";
    /// Empty.
    NONE_FOUND = "setup.none_found";
    /// Truncated.
    TRUNCATED = "setup.truncated";
    /// Toggle.
    ADD_SEVERAL = "setup.add_several";
    /// Disclosure.
    HIDE_FILTERED = "setup.hide_filtered";
    /// Button.
    CHOOSE_FILE = "setup.choose_file";
    /// Button.
    CONTINUE = "setup.continue";
    /// Step 1: what to do.
    CHOOSE_TEXT = "setup.choose_text";
    /// Step 1: the drop area's first line.
    DROP_HERE = "setup.drop_here";
    /// Step 1: the drop area's second line.
    OR_PICK = "setup.or_pick";
    /// Step 2: why Continue waits after it was pressed.
    SETTING_UP = "setup.setting_up";
    /// Step 2 and 3: back to the step before.
    BACK = "setup.back";
}

/// Step 2: "Programs found in “Celeste”. Pick the one that starts the game."
pub fn found_text(folder: &str) -> String {
    rust_i18n::t!("setup.found_text", folder = folder).into_owned()
}

/// "Show 2 uninstallers and helpers".
pub fn show_filtered(n: usize) -> String {
    rust_i18n::t!("setup.show_filtered", n = n).into_owned()
}

/// "Programs found in “Celeste”".
pub fn found_in(folder: &str) -> String {
    rust_i18n::t!("setup.found_in", folder = folder).into_owned()
}

/// Role guess.
pub fn role_guess(role: zaun_core::model::ExeRole, filtered: bool) -> &'static str {
    use zaun_core::model::ExeRole as R;
    match (role, filtered) {
        (R::Tool, true) => crate::i18n::text("setup.role_guess.uninstaller_or_helper"),
        (R::Installer, _) => crate::i18n::text("setup.role_guess.installer"),
        (R::Primary, _) => crate::i18n::text("setup.role_guess.main_program"),
        (R::Tool, false) | (R::Launcher, _) => {
            crate::i18n::text("setup.role_guess.looks_like_a_tool")
        }
        (R::Alternate, _) => crate::i18n::text("setup.role_guess.another_version"),
    }
}

/// "Celeste.exe, main program, 32 MB".
pub fn program_row(file: &str, role: &str, size: u64) -> String {
    rust_i18n::t!(
        "setup.program_row",
        file = file,
        role = role.to_lowercase(),
        size = super::fmt::bytes(size)
    )
    .into_owned()
}

// ---- Quick launch ------------------------------------------------------------------------

texts! {
    /// Button.
    OPEN_ZAUN = "setup.open_zaun";
    /// Button.
    DETAILS = "setup.details";
}

/// "Game folder: Games › Celeste".
pub fn folder_button(label: &str) -> String {
    rust_i18n::t!("setup.folder_button", folder = GAME_FOLDER(), label = label).into_owned()
}

/// "Main program, 32 MB".
pub fn role_size(role: &str, size: u64) -> String {
    rust_i18n::t!(
        "setup.role_size",
        role = role,
        size = super::fmt::bytes(size)
    )
    .into_owned()
}

// ---- Set up this game (step 3 of "Add a game") -----------------------------------------

texts! {
    /// Label of the protection control.
    PROTECTION = "setup.protection";
    /// The poster of a game whose name is still empty.
    UNTITLED = "setup.untitled";
}

/// The cover preview, spoken: "Cover preview: Celeste".
pub fn cover_preview(title: &str) -> String {
    rust_i18n::t!("setup.cover_preview", title = title).into_owned()
}
texts! {
    /// The left action of step 3 when there is no step before it.
    CANCEL = "setup.cancel";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_and_size_read_as_one_phrase() {
        assert_eq!(role_size("Main program", 32_000_000).split(", ").count(), 2);
    }
}
