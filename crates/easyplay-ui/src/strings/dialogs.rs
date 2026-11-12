//! Dialogs (SCREEN_SPECS_2 §16.1) and prompts (LOGIC_SPECS §4).

use easyplay_core::model::BasePreset;

/// Button.
pub const CANCEL: &str = "Cancel";
/// Button.
pub const CLOSE: &str = "Close";
/// Button.
pub const REMOVE: &str = "Remove game";
/// Button.
pub const RESET: &str = "Reset";
/// Button.
pub const KEEP_EDITING: &str = "Keep editing";
/// Button.
pub const DISCARD: &str = "Discard";
/// Button.
pub const ALLOW: &str = "Allow";
/// Button.
pub const REMOVE_LAYER: &str = "Remove";
/// Button.
pub const CLEAR: &str = "Clear history";
/// Button.
pub const TURN_ON: &str = "Turn on";
/// Button.
pub const SET_UP_NEW: &str = "Set up as new";
/// Button.
pub const ITS_AN_UPDATE: &str = "It's an update";
/// Button.
pub const RUN_WITH_SETTINGS: &str = "Run with the game's settings";
/// Button.
pub const USE_SAME: &str = "Use the same settings";
/// Button.
pub const ALLOW_NEW_LOCATION: &str = "Allow the new location";
/// Button.
pub const USE_DEFAULT: &str = "Use the default layer";
/// Button.
pub const NOT_NOW: &str = "Not now";
/// Button.
pub const TURN_OFF_FIXES: &str = "Turn off game fixes";
/// Button.
pub const ALLOW_ONCE: &str = "Allow once";
/// Button.
pub const KEEP_AS_INSTALLER: &str = "Keep as installer";
/// Button.
pub const REMOVE_ENTRY: &str = "Remove from library";
/// Button.
pub const USE_PROGRAM: &str = "Use this program";
/// Button.
pub const SHARE_ANYWAY: &str = "Share anyway";
/// Button.
pub const STOP_AND_REMOVE: &str = "Stop and remove";
/// Button.
pub const COPY: &str = "Copy";
/// Disclosure.
pub const SHOW_DETAILS: &str = "Details";
/// Disclosure.
pub const HIDE_DETAILS: &str = "Hide details";
/// Remove body.
pub const REMOVE_BODY: &str = "The game's own files stay where they are.";
/// Option.
pub const REMOVE_STORAGE: &str = "Delete its private Windows storage";
/// Option hint.
pub const REMOVE_STORAGE_HINT: &str = "Settings the game saved in its Windows storage are deleted.";
/// Option.
pub const KEEP_SAVES: &str = "Keep save data";
/// Option.
pub const REMOVE_SHORTCUTS: &str = "Remove menu shortcuts";
/// Option.
pub const KEEP_HISTORY: &str = "Keep play history";
/// Reset lists.
pub const RESET_DELETED: &str = "Deleted:";
/// Reset lists.
pub const RESET_DELETED_1: &str = "Windows settings and installed extras";
/// Reset lists.
pub const RESET_DELETED_2: &str = "Files the game wrote outside its folder";
/// Reset lists.
pub const RESET_KEPT: &str = "Kept:";
/// Reset lists.
pub const RESET_KEPT_1: &str = "The game's own folder and play history";
/// Discard.
pub const DISCARD_TITLE: &str = "Discard your changes?";
/// Discard.
pub const DISCARD_BODY: &str = "The changes you made on this page haven't been saved.";
/// Widen.
pub const WIDEN_BODY: &str = "It will be able to:";
/// Clear statistics.
pub const CLEAR_TITLE: &str = "Clear all play history?";
/// Developer Mode.
pub const DEV_TITLE: &str = "Turn on Developer Mode?";
/// Developer Mode.
pub const DEV_BODY: &str = "Developer Mode shows logs, the launch inspector and technical options.";
/// Developer Mode.
pub const DEV_UNSAFE: &str =
    "Options marked unsafe can stop games from starting or weaken protection.";
/// Exe changed.
pub const EXE_CHANGED_BODY: &str = "If you just updated it, that's expected.";
/// Runtime missing.
pub const RUNTIME_MISSING_BODY: &str =
    "The compatibility layer this game uses is no longer installed.";
/// Fix needs network.
pub const FIX_NETWORK_TITLE: &str = "A game fix needs the internet";
/// Tool needs network.
pub const TOOL_NETWORK_TITLE: &str = "Let this tool use the internet once?";
/// Tainted prefix.
pub const TAINTED: &str = "This game has already run in this storage, so anything it left there could also use the connection.";
/// Installer finished.
pub const INSTALLER_TITLE: &str = "Which program is the game?";
/// Grant moved.
pub const GRANT_MOVED_TITLE: &str = "A shared folder moved";
/// Broad grant.
pub const BROAD_REMINDER: &str = "The game will be able to read everything in it.";
/// Removing a running game.
pub const RUNNING_REMOVE_BODY: &str =
    "It will be stopped first. Unsaved progress in the game may be lost.";
/// Remember choice.
pub const REMEMBER: &str = "Remember for this file";

/// "Remove Hollow Knight from EasyPlay?"
pub fn remove_title(name: &str) -> String {
    format!("Remove {name} from EasyPlay?")
}
/// "Remove 3 games from EasyPlay?"
pub fn remove_many_title(n: usize) -> String {
    format!("Remove {n} games from EasyPlay?")
}
/// "Reset private Windows storage for Hollow Knight?"
pub fn reset_title(name: &str) -> String {
    format!("Reset private Windows storage for {name}?")
}
/// "Give Hollow Knight internet access?"
pub fn widen_title(name: &str, p: BasePreset) -> String {
    match p {
        BasePreset::Standard => format!("Switch {name} to Standard protection?"),
        _ => format!("Give {name} internet access?"),
    }
}
/// "Give 3 games internet access?"
pub fn widen_many_title(n: usize, p: BasePreset) -> String {
    match p {
        BasePreset::Standard => format!("Switch {n} games to Standard protection?"),
        _ => format!("Give {n} games internet access?"),
    }
}
/// The statements a wider preset adds.
pub fn widen_statements(p: BasePreset) -> Vec<&'static str> {
    match p {
        BasePreset::Offline => vec![],
        BasePreset::Online => vec!["Use the internet and your local network"],
        BasePreset::Standard => vec![
            "Use the internet and your local network",
            "Read all input devices, where your system allows it",
            "Show windows with fewer restrictions",
        ],
    }
}
/// "1 running game is skipped."
pub fn skipped_running(n: usize) -> String {
    format!(
        "{n} running {} skipped.",
        super::plural(n as u64, "game is", "games are")
    )
}
/// "Remove GE-Proton 10-25?"
pub fn remove_layer_title(label: &str) -> String {
    format!("Remove {label}?")
}
/// "Frees 1.4 GB."
pub fn frees(bytes: u64) -> String {
    format!("Frees {}.", super::fmt::bytes(bytes))
}
/// "No games use it." / "3 games use it; they switch to the default."
pub fn layer_users(n: u32) -> String {
    match n {
        0 => "No games use it.".to_owned(),
        n => format!(
            "{n} {} it; they will use the default layer.",
            super::plural(u64::from(n), "game uses", "games use")
        ),
    }
}
/// "Clear play history for Hollow Knight?"
pub fn clear_game_title(name: &str) -> String {
    format!("Clear play history for {name}?")
}
/// "57 sessions and 41 h 20 min of play will be deleted."
pub fn clear_body(sessions: u64, ms: u64) -> String {
    format!(
        "{sessions} {} and {} of play will be deleted.",
        super::plural(sessions, "session", "sessions"),
        super::fmt::duration(ms)
    )
}
/// "Hollow Knight looks different"
pub fn exe_changed_title(name: &str) -> String {
    format!("{name} looks different")
}
/// "Was 32 MB, now 33 MB."
pub fn exe_changed_sizes(old: u64, new: u64) -> String {
    format!(
        "Was {}, now {}.",
        super::fmt::bytes(old),
        super::fmt::bytes(new)
    )
}
/// "Run Config.exe with Hollow Knight's settings?"
pub fn inside_root_title(file: &str, name: &str) -> String {
    format!("Run {file} with {name}'s settings?")
}
/// Body.
pub fn inside_root_body(name: &str) -> String {
    format!(
        "This program is inside {name}'s folder. It can use the same protection and storage, or be set up as its own game."
    )
}
/// "This is a copy of Hollow Knight"
pub fn copy_title(name: &str) -> String {
    format!("This is a copy of {name}")
}
/// Body.
pub fn copy_body(file: &str, name: &str) -> String {
    format!("{file} is identical to {name}'s program in another place.")
}
/// Body.
pub fn grant_moved_body(name: &str, folder: &str, new: &str) -> String {
    format!("{name} may use “{folder}”, which now points to {new}.")
}
/// "The compatibility layer for Hollow Knight is missing"
pub fn runtime_missing_title(name: &str) -> String {
    format!("The compatibility layer for {name} is missing")
}
/// Body.
pub fn fix_network_body(name: &str) -> String {
    format!("A fix for {name} downloads a component from the internet before it starts.")
}
/// Body.
pub fn tool_network_body(name: &str) -> String {
    format!("This tool downloads components into {name}'s private Windows storage.")
}
/// Body.
pub fn installer_body(name: &str) -> String {
    format!("{name} finished installing. Choose the program to start from now on.")
}
/// "Celeste.exe · 32 MB".
pub fn program_choice(label: &str, size: u64) -> String {
    format!("{label} · {}", super::fmt::bytes(size))
}
/// "Share all of Documents?"
pub fn broad_title(folder: &str) -> String {
    format!("Share all of {folder}?")
}
/// The reason, from a catalogue key.
pub fn broad_reason(key: &str) -> String {
    match key {
        "mount_root" => "This is an entire drive.".to_owned(),
        "user_dir" => "This is one of your main personal folders.".to_owned(),
        _ => "This folder holds a large number of files.".to_owned(),
    }
}
/// "Hollow Knight is running"
pub fn running_remove_title(name: &str) -> String {
    format!("{name} is running")
}
/// "1 more problem".
pub fn more_problems(n: usize) -> String {
    format!(
        "{n} more {}",
        super::plural(n as u64, "problem", "problems")
    )
}
