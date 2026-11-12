//! First-time setup sheet (SCREEN_SPECS §6) and the add-game sheet (§5).

use std::path::Path;

use easyplay_core::model::BasePreset;

/// Sheet title.
pub const TITLE: &str = "Set up this game";
/// Title after Play.
pub const STARTING_TITLE: &str = "Starting";
/// Field.
pub const NAME: &str = "Name";
/// Validation.
pub const NAME_REQUIRED: &str = "A name is required (up to 80 characters).";
/// Label.
pub const GAME_FOLDER: &str = "Game folder";
/// Option.
pub const FILE_ONLY: &str = "This file only";
/// Option.
pub const CHOOSE_FOLDER: &str = "Choose folder…";
/// Loose file explanation.
pub const LOOSE_FILE: &str =
    "This program is not in a folder of its own, so the game can read only this file.";
/// Summary heading.
pub const WILL_BE_ABLE: &str = "This game will be able to:";
/// Honesty line.
pub const HONESTY: &str = "Protection limits what a program can reach. It reduces risk; it cannot make untrusted software harmless.";
/// Link.
pub const LEARN_MORE: &str = "Learn more";
/// Popover.
pub const HONESTY_MORE: &str = "EasyPlay runs each game in a private space with only the access listed here. A program that finds a flaw in your system could still escape, so run software you have reason to trust.";
/// Hint.
pub const INSTALLER_HINT: &str =
    "This looks like an installer. EasyPlay will run it, then ask which installed program to keep.";
/// Hint.
pub const ONLINE_HINT: &str =
    "This game can probably go online. Choose Online if sign-in or multiplayer doesn't work.";
/// Acknowledgement.
pub const I_UNDERSTAND: &str = "I understand";
/// Disclosure.
pub const MORE_OPTIONS: &str = "More options";
/// Section.
pub const FILES: &str = "Files and folders";
/// Section.
pub const LAUNCH: &str = "Launch";
/// Section.
pub const TYPE: &str = "Type";
/// Option.
pub const KIND_GAME: &str = "Game";
/// Option.
pub const KIND_INSTALLER: &str = "Installer";
/// Option.
pub const KIND_TOOL: &str = "Tool";
/// Button.
pub const ADD_TO_LIBRARY: &str = "Add to library";
/// Button.
pub const USE_FOR_REST: &str = "Use these settings for the rest";
/// Button.
pub const RUN_IN_BACKGROUND: &str = "Run in background";
/// Disabled reason.
pub const FIX_TO_CONTINUE: &str = "Fix the marked problem to continue";

/// "1 of 3".
pub fn queue(i: u32, n: u32) -> String {
    format!("{i} of {n}")
}

/// "Games ▸ Hollow Knight".
pub fn breadcrumb(p: &Path, home: &Path) -> String {
    let rel = p.strip_prefix(home).unwrap_or(p);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|c| c != "/")
        .collect::<Vec<_>>()
        .join(" › ")
}

/// "Online: can use the internet and your local network".
pub fn announce_preset(b: BasePreset, first: Option<&str>) -> String {
    let name = super::shield::preset_name(b.into());
    match first {
        Some(f) => format!("{name}: {}", f.to_lowercase()),
        None => name.to_owned(),
    }
}

// ---- Add game ----------------------------------------------------------------------------

/// Sheet.
pub const ADD_TITLE: &str = "Add game";
/// Scanning.
pub const LOOKING: &str = "Looking for programs…";
/// Empty.
pub const NONE_FOUND: &str = "No Windows programs found in this folder.";
/// Truncated.
pub const TRUNCATED: &str = "Showing the first 2,000 files' worth of results.";
/// Toggle.
pub const ADD_SEVERAL: &str = "Add several";
/// Disclosure.
pub const SHOW_FILTERED: &str = "Show uninstallers and helpers";
/// Disclosure.
pub const HIDE_FILTERED: &str = "Hide uninstallers and helpers";
/// Manual choice.
pub const NOT_HERE: &str = "Not here?";
/// Button.
pub const CHOOSE_FILE: &str = "Choose a file…";
/// Button.
pub const CONTINUE: &str = "Continue";
/// List name.
pub const PROGRAMS: &str = "Programs";

/// "Programs found in “Celeste”".
pub fn found_in(folder: &str) -> String {
    format!("Programs found in “{folder}”")
}

/// Role guess.
pub fn role_guess(role: easyplay_core::model::ExeRole, filtered: bool) -> &'static str {
    use easyplay_core::model::ExeRole as R;
    match (role, filtered) {
        (R::Tool, true) => "Uninstaller or helper",
        (R::Installer, _) => "Installer",
        (R::Primary, _) => "Main program",
        (R::Tool, false) | (R::Launcher, _) => "Looks like a tool",
        (R::Alternate, _) => "Another version",
    }
}

/// "Celeste.exe, main program, 32 MB".
pub fn program_row(file: &str, role: &str, size: u64) -> String {
    format!(
        "{file}, {}, {}",
        role.to_lowercase(),
        super::fmt::bytes(size)
    )
}

// ---- Quick launch ------------------------------------------------------------------------

/// Button.
pub const OPEN_EASYPLAY: &str = "Open EasyPlay";
/// Button.
pub const DETAILS: &str = "Details";
/// Already running.
pub const ALREADY_RUNNING: &str = "Already running";
