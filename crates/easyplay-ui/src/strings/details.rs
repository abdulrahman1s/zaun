//! The game page (SCREEN_SPECS §4, SCREEN_SPECS_2 §8–11).

use std::path::Path;

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Availability, BasePreset, SandboxPolicy};

use crate::vm::details::DetailsTab;

/// Sample tool label in preview data.
pub const SAMPLE_TOOL_LABEL: &str = "Settings";
/// Sample note in preview data.
pub const SAMPLE_NOTE: &str = "Finished the main story. Try the hard mode next.";

/// Tab list name.
pub const TABS: &str = "Game page sections";

/// Tab names.
pub fn tab(t: DetailsTab) -> &'static str {
    match t {
        DetailsTab::Overview => "Overview",
        DetailsTab::Protection => "Protection",
        DetailsTab::Compatibility => "Compatibility",
        DetailsTab::LaunchOptions => "Launch options",
        DetailsTab::Activity => "Activity",
        DetailsTab::Logs => "Logs",
    }
}

/// Cover button.
pub const CHANGE_PICTURE: &str = "Cover: change picture";
/// Title tooltip.
pub const RENAME_TOOLTIP: &str = "Click or press F2 to rename";
/// Rename field.
pub const RENAME_FIELD: &str = "Name";
/// Rename field description.
pub const RENAME_HINT: &str = "Enter saves, Esc cancels";
/// Stopping notice.
pub const STOPPING_BODY: &str = "Waiting for the game to close. Force stop ends it at once.";
/// Availability action.
pub const CHECK_AGAIN: &str = "Check again";
/// Tamper notice.
pub const TAMPER: &str = "Shared files changed after this game ran";
/// Tamper notice.
pub const TAMPER_BODY: &str = "Files of the compatibility layer were modified while this game could reach them. They are checked again before the next start.";

/// Availability notice title and body.
pub fn availability(a: Availability) -> (&'static str, &'static str) {
    match a {
        Availability::DriveDisconnected => (
            "The drive with this game isn't connected",
            "Connect the drive and check again, or locate the program if it moved.",
        ),
        Availability::Unreadable => (
            "This game can't be read",
            "EasyPlay isn't allowed to read its folder. Check the folder's permissions.",
        ),
        _ => (
            "This game's program can't be found",
            "It may have been moved, renamed or deleted. Locate it to keep your settings and playtime.",
        ),
    }
}

/// "Last played yesterday · 12 h 05 min".
pub fn played_line(
    total_ms: u64,
    last: Option<Timestamp>,
    now: Timestamp,
    offset_s: i32,
) -> String {
    match last {
        Some(t) if total_ms > 0 => format!(
            "Last played {} · {}",
            super::fmt::relative_inline(t, now, offset_s),
            super::fmt::duration(total_ms)
        ),
        _ => super::card::NOT_PLAYED.to_owned(),
    }
}

// ---- Overview ----------------------------------------------------------------------------

/// Card.
pub const REACH: &str = "What this game can reach";
/// Link.
pub const SEE_EVERYTHING: &str = "See everything";
/// Card.
pub const PLAYTIME: &str = "Playtime";
/// Caption.
pub const LAST_7_DAYS: &str = "Last 7 days";
/// Card.
pub const DETAILS: &str = "Details";
/// Row.
pub const LOCATION: &str = "Location";
/// Row.
pub const SAVE_DATA: &str = "Save data";
/// Row.
pub const ADDED: &str = "Added";
/// Row.
pub const PROGRAMS: &str = "Programs";
/// Button.
pub const OPEN: &str = "Open";
/// Value.
pub const PRIVATE_STORAGE: &str = "Private storage";
/// Value.
pub const YOUR_FOLDER: &str = "A folder you chose";
/// Card.
pub const RECENT_SESSIONS: &str = "Recent sessions";
/// Link.
pub const SEE_ALL: &str = "See all";
/// Card.
pub const NOTES: &str = "Notes";
/// Hint.
pub const ADD_NOTE: &str = "Add a note";
/// Query failure.
pub const COULDNT_LOAD: &str = "Couldn't load this.";
/// Button.
pub const RETRY: &str = "Retry";

/// "31 sessions".
pub fn sessions(n: u32) -> String {
    format!("{n} {}", super::plural(u64::from(n), "session", "sessions"))
}

/// ("Hollow Knight", Some("in Games")) for a game folder; the program's file otherwise.
pub fn location(root: Option<&Path>, exe: &Path) -> (String, Option<String>) {
    let p = root.unwrap_or(exe);
    let name = p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let parent = p
        .parent()
        .and_then(|x| x.file_name())
        .map(|n| format!("in {}", n.to_string_lossy()));
    (name, parent)
}

// ---- Save bar ----------------------------------------------------------------------------

/// Save bar.
pub const UNSAVED: &str = "Unsaved changes";
/// Save bar when access widens.
pub const UNSAVED_WIDENS: &str =
    "Unsaved changes — this program and anything it has installed will get this access.";
/// Button.
pub const SAVE: &str = "Save";
/// Button.
pub const DISCARD: &str = "Discard";
/// Disabled reason.
pub const FIX_LAUNCH: &str = "Fix the launch options to save";

/// "Fix 1 problem to save".
pub fn fix_problems(n: usize) -> String {
    format!(
        "Fix {n} {} to save",
        super::plural(n as u64, "problem", "problems")
    )
}

// ---- Protection ----------------------------------------------------------------------------

/// Heading and radio group.
pub const PROTECTION: &str = "Protection";
/// Link.
pub const RESET_TO_PRESET: &str = "Reset to preset";
/// Inline confirm button.
pub const REPLACE: &str = "Replace";
/// Banner while running.
pub const APPLIES_NEXT_TIME: &str = "Changes apply the next time you start the game.";
/// Policy of a newer version.
pub const POLICY_TOO_NEW: &str =
    "These settings were saved by a newer version of EasyPlay and can't be shown here.";
/// Summary marker.
pub const ADDED_MARK: &str = "added";
/// Summary marker.
pub const REMOVED_MARK: &str = "removed";
/// Section.
pub const INTERNET: &str = "Internet";
/// Option.
pub const NO_INTERNET: &str = "No internet";
/// Option.
pub const INTERNET_ON: &str = "Internet and local network";
/// Section.
pub const FILES: &str = "Files and folders";
/// Row.
pub const OWN_FOLDER: &str = "Its own folder";
/// Row.
pub const YOUR_FOLDERS: &str = "Your folders";
/// Empty list.
pub const NO_FOLDERS: &str =
    "This game can't see any of your folders. Add one if it needs your music, mods or screenshots.";
/// Access.
pub const CAN_READ: &str = "Can read";
/// Access.
pub const CAN_CHANGE: &str = "Can change";
/// Access.
pub const NOT_SHARED: &str = "Not shared";
/// Button.
pub const ADD_FOLDER: &str = "Add folder…";
/// Drop hint.
pub const DROP_FOLDER: &str = "Drop a folder here to share it";
/// Row.
pub const REMOVABLE: &str = "Removable drives";
/// Note.
pub const WRITABLE_WITH_INTERNET: &str =
    "With internet access, a program can upload or alter these files.";
/// Option.
pub const CHOOSE_FOLDER: &str = "A folder I choose…";
/// Section.
pub const DEVICES: &str = "Devices";
/// Toggle.
pub const SOUND: &str = "Sound (speakers and microphone)";
/// Row.
pub const CONTROLLERS: &str = "Controllers";
/// Option.
pub const CTRL_NONE: &str = "None";
/// Option.
pub const CTRL_CONNECTED: &str = "Connected";
/// Option.
pub const CTRL_ALL: &str = "All input devices";
/// Disabled reason.
pub const ALL_INPUT_UNAVAILABLE: &str =
    "All input devices is unavailable because your account can read keyboards directly.";
/// Toggle.
pub const CAMERA: &str = "Camera";
/// Section.
pub const DISPLAY: &str = "Display";
/// X11 notice.
pub const X11_NOTICE: &str = "Shows windows through X11 — other X11 apps are not isolated from it.";
/// Advanced.
pub const GAME_FOLDER_ACCESS: &str = "Game folder access";
/// Option.
pub const READ_ONLY: &str = "Read only";
/// Option.
pub const PROTECTED: &str = "Protected";
/// Loose file.
pub const FILE_ONLY_EXPLAINED: &str =
    "This game is a single file, so it can only read its own program.";
/// Advanced.
pub const IDENTITY: &str = "Hide this computer's identity";
/// Toggle.
pub const MASK_HOSTNAME: &str = "Hide the computer's name";
/// What breaks.
pub const MASK_HOSTNAME_BREAKS: &str = "Rarely matters. Some local multiplayer games show it.";
/// Toggle.
pub const MASK_MACHINE_ID: &str = "Hide the machine ID";
/// What breaks.
pub const MASK_MACHINE_ID_BREAKS: &str =
    "Some copy protection checks may ask you to activate again.";
/// Toggle.
pub const MASK_HARDWARE: &str = "Hide hardware serial numbers";
/// What breaks.
pub const MASK_HARDWARE_BREAKS: &str = "Some copy protection checks may ask you to activate again.";
/// Toggle.
pub const SECCOMP: &str = "Filter risky system calls";
/// Hint.
pub const SECCOMP_HINT: &str = "Turn off only if a game fails with a message about a blocked call.";
/// Advanced.
pub const LAYER_ISOLATION: &str = "Compatibility layer copy";
/// Option.
pub const ISO_AUTO: &str = "Automatic";
/// Option.
pub const ISO_PRIVATE: &str = "Always a private copy";
/// Option.
pub const ISO_SHARED: &str = "Shared";
/// Toggle.
pub const DIRECT_SOUND: &str = "Direct access to sound devices";
/// Hint.
pub const DIRECT_SOUND_HINT: &str = "Only for old games that cannot use the sound server.";

/// "Custom (based on Online)".
pub fn custom_based_on(b: BasePreset) -> String {
    format!("Custom (based on {})", super::shield::preset_name(b.into()))
}

/// "Choosing Online replaces your changes."
pub fn replace_custom(b: BasePreset) -> String {
    format!(
        "Choosing {} replaces your changes.",
        super::shield::preset_name(b.into())
    )
}

/// "Share Music".
pub fn share_toggle(name: &str) -> String {
    format!("Share {name}")
}

/// "This folder can't be shared: …".
pub fn rejected(reason: &str) -> String {
    format!("This folder can't be shared: {reason}")
}

/// "in Home", "in Documents ▸ My Games".
pub fn folder_location(path: &Path, home: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    if parent == home {
        return "in Home".to_owned();
    }
    match parent.strip_prefix(home) {
        Ok(rel) => format!(
            "in {}",
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" › ")
        ),
        Err(_) => format!("in {}", parent.display()),
    }
}

/// "Music, in Home, can read. Press left or right to change access, Delete to remove."
pub fn folder_row_name(name: &str, loc: &str, writable: bool) -> String {
    format!(
        "{name}, {loc}, {}. Press left or right to change access, Delete to remove.",
        if writable { "can change" } else { "can read" }
    )
}

/// "Remove Music".
pub fn remove_folder(name: &str) -> String {
    format!("Remove {name}")
}

/// "Advanced · 1 changed".
pub fn advanced(changed: usize) -> String {
    if changed == 0 {
        "Advanced".to_owned()
    } else {
        format!("Advanced · {changed} changed")
    }
}

/// How many advanced fields differ.
pub fn advanced_changes(a: &SandboxPolicy, b: &SandboxPolicy) -> usize {
    usize::from(a.game_dir != b.game_dir)
        + usize::from(a.hardening.mask_hostname != b.hardening.mask_hostname)
        + usize::from(a.hardening.mask_machine_id != b.hardening.mask_machine_id)
        + usize::from(a.hardening.mask_hardware_ids != b.hardening.mask_hardware_ids)
        + usize::from(a.hardening.seccomp != b.hardening.seccomp)
        + usize::from(a.hardening.runtime_isolation != b.hardening.runtime_isolation)
        + usize::from(a.devices.alsa_devices != b.devices.alsa_devices)
}

// ---- Compatibility ------------------------------------------------------------------------

/// Section.
pub const LAYER: &str = "Compatibility layer";
/// Group.
pub const GROUP_DEFAULT: &str = "Default";
/// Group.
pub const GROUP_CHANNELS: &str = "Channels";
/// Group.
pub const GROUP_INSTALLED: &str = "Installed — EasyPlay";
/// Group.
pub const GROUP_OTHER: &str = "Installed — other sources";
/// Checkbox.
pub const FOLLOW_UPDATES: &str = "Follow updates";
/// No layer yet.
pub const NOT_DOWNLOADED: &str = "not downloaded yet";
/// Missing layer.
pub const LAYER_MISSING: &str = "This layer is no longer installed.";
/// Older layer.
pub const LAYER_OLDER: &str = "This layer is older than the one that prepared the game's storage. If the game misbehaves, reset the storage.";
/// Button.
pub const USE_DEFAULT: &str = "Use default";
/// Empty catalogue.
pub const NONE_INSTALLED: &str = "Default — will be downloaded on first start.";
/// Button.
pub const DOWNLOAD_NOW: &str = "Download now";
/// Section.
pub const FIXES: &str = "Game fixes";
/// Layer without fixes.
pub const NO_FIXES_IN_LAYER: &str = "This layer doesn't use game fixes.";
/// No fix.
pub const NO_FIX: &str = "No fix selected";
/// Button.
pub const CHANGE: &str = "Change…";
/// Toggle.
pub const TURN_OFF_FIXES: &str = "Turn off fixes";
/// Section.
pub const OPTIONS: &str = "Options";
/// Row.
pub const GRAPHICS: &str = "Graphics";
/// Row.
pub const WINDOW_SYSTEM: &str = "Window system";
/// Option.
pub const AUTOMATIC: &str = "Automatic";
/// Row.
pub const OVERLAY: &str = "Performance overlay";
/// Note.
pub const NOT_INSTALLED: &str = "not installed on this system";
/// Row.
pub const LANGUAGE: &str = "Game language";
/// Option.
pub const LANG_SYSTEM: &str = "Same as system";
/// Disclosure.
pub const ADVANCED_COMPAT: &str = "Advanced";
/// Row.
pub const SYNC: &str = "Synchronisation";
/// Option.
pub const NO_FSYNC: &str = "No fsync";
/// Option.
pub const NO_ESYNC: &str = "No esync or fsync";
/// Field.
pub const DLL_OVERRIDES: &str = "DLL overrides";
/// Field.
pub const DXVK_HUD: &str = "DXVK overlay";
/// Toggle.
pub const PROTON_LOG: &str = "Write a Proton log";
/// Hint.
pub const PROTON_LOG_HINT: &str = "Large files; turn on only to report a problem.";
/// Card.
pub const STORAGE: &str = "Private Windows storage";
/// Size placeholder.
pub const CALCULATING: &str = "Calculating…";
/// Button.
pub const OPEN_FOLDER: &str = "Open folder";
/// Button.
pub const WINDOWS_SETTINGS: &str = "Windows settings";
/// Button.
pub const RESET: &str = "Reset…";
/// Sheet.
pub const FIX_PICKER: &str = "Choose a game fix";
/// Field.
pub const FIX_SEARCH: &str = "Search by title, store or ID";
/// Empty.
pub const NO_FIX_FOUND: &str = "No fix found for this name. Most games don't need one.";
/// Button.
pub const NO_FIX_USE: &str = "Use no fix";

/// "Default — UMU-Proton 10.0-4".
pub fn default_layer(resolved: Option<&str>) -> String {
    match resolved {
        Some(r) => format!("Default — {r}"),
        None => NONE_INSTALLED.to_owned(),
    }
}

/// "UMU-Proton (latest)".
pub fn channel(name: &str) -> String {
    format!("{name} (latest)")
}

/// "Last used: … · storage prepared with the same version".
pub fn status_line(used: &str, prepared: Option<&str>, older: bool) -> String {
    match prepared {
        Some(_) if older => format!("Last used: {used} · storage prepared with a newer version"),
        Some(p)
            if used.replace(' ', "-").eq_ignore_ascii_case(p) || p.replace('-', " ") == used =>
        {
            format!("Last used: {used} · storage prepared with the same version")
        }
        Some(p) => format!("Last used: {used} · storage prepared with {p}"),
        None => format!("Last used: {used}"),
    }
}

/// "2.3 GB · created 2 Nov".
pub fn storage_line(size: &str, created: Option<&str>) -> String {
    match created {
        Some(c) => format!("{size} · created {c}"),
        None => size.to_owned(),
    }
}

/// "Hollow Knight (GOG) · umu-367520".
pub fn fix_row(title: &str, store: &str, id: &str) -> String {
    format!("{title} ({store}) · {id}")
}

// ---- Launch options ----------------------------------------------------------------------

/// Card.
pub const PROGRAMS_TITLE: &str = "Programs";
/// Role.
pub const ROLE_PRIMARY: &str = "Main program";
/// Role.
pub const ROLE_ALTERNATE: &str = "Other version";
/// Role.
pub const ROLE_LAUNCHER: &str = "Launcher";
/// Role.
pub const ROLE_TOOL: &str = "Tool";
/// Role.
pub const ROLE_INSTALLER: &str = "Installer";
/// Menu.
pub const MAKE_MAIN: &str = "Make main program";
/// Menu.
pub const REMOVE_PROGRAM: &str = "Remove from this game";
/// Button.
pub const ADD_PROGRAM: &str = "Add program…";
/// Field.
pub const ARGUMENTS: &str = "Arguments";
/// Help.
pub const ARGUMENTS_HELP: &str = "Passed to the program when it starts.";
/// Error.
pub const ARGS_UNBALANCED: &str = "A quote is not closed.";
/// Section.
pub const START_IN: &str = "Start in";
/// Option.
pub const WD_PROGRAM: &str = "Program's folder";
/// Option.
pub const WD_GAME: &str = "Game folder";
/// Option.
pub const WD_OTHER: &str = "Other…";
/// Section.
pub const WHEN_STOPPING: &str = "When stopping, wait";
/// After the select.
pub const BEFORE_FORCING: &str = "before forcing";
/// Section.
pub const ENV_VARS: &str = "Environment variables";
/// Empty.
pub const NO_VARS: &str = "No variables set.";
/// Button.
pub const ADD_VAR: &str = "Add variable";
/// Rule.
pub const DEV_ONLY_VAR: &str = "Only available in Developer Mode";
/// Rule.
pub const INVALID_VAR: &str = "Use letters, digits and underscores, not starting with a digit.";

/// "Tool · “Settings”".
pub fn role_with_label(role: &str, label: Option<&str>) -> String {
    match label {
        Some(l) => format!("{role} · “{l}”"),
        None => role.to_owned(),
    }
}

/// "Actions for Config.exe".
pub fn program_menu(file: &str) -> String {
    format!("Actions for {file}")
}

/// "Run Config.exe".
pub fn run_program(file: &str) -> String {
    format!("Run {file}")
}

/// "Mark as Tool".
pub fn mark_as(role: &str) -> String {
    format!("Mark as {role}")
}

/// "3 arguments".
pub fn args_preview(n: usize) -> String {
    format!("{n} {}", super::plural(n as u64, "argument", "arguments"))
}

/// "10 s".
pub fn seconds(n: u8) -> String {
    format!("{n} s")
}

/// "PATH is managed by EasyPlay".
pub fn reserved_var(name: &str) -> String {
    format!("{name} is managed by EasyPlay (Compatibility and Protection settings)")
}

/// "Variable name, row 1".
pub fn var_name(row: usize) -> String {
    format!("Variable name, row {row}")
}

/// "Value, row 1".
pub fn var_value(row: usize) -> String {
    format!("Value, row {row}")
}

/// "Remove variable 1".
pub fn remove_var(row: usize) -> String {
    format!("Remove variable {row}")
}

// ---- Activity ----------------------------------------------------------------------------

/// Empty.
pub const NO_SESSIONS: &str = "No sessions yet.";
/// Heading.
pub const HISTORY: &str = "History";
/// Button.
pub const LOAD_MORE: &str = "Load more";
/// Menu.
pub const REMOVE_SESSION: &str = "Remove from history";
/// Link.
pub const RESET_STATS: &str = "Reset statistics for this game…";

/// "Compatibility layer: Default — UMU-Proton 10.0-4".
pub fn layer_button(label: &str) -> String {
    format!("{LAYER}: {label}")
}

/// Folder row state when the folder is gone (SCREEN_SPECS_2 §9).
pub const FOLDER_NOT_FOUND: &str = "Not found";
/// Button on a missing folder row.
pub const LOCATE_FOLDER: &str = "Locate…";

/// Program menu item.
pub const RENAME_LABEL: &str = "Rename label…";
/// Program menu item.
pub const PROGRAM_ARGS: &str = "Arguments for this program…";
/// Inline editor field.
pub const LABEL_FIELD: &str = "Label";
/// Inline editor help.
pub const LABEL_HELP: &str = "Shown next to the program. Leave empty for no label.";
/// Inline editor field.
pub const PROGRAM_ARGS_FIELD: &str = "Arguments for this program";
/// Inline editor help.
pub const PROGRAM_ARGS_HELP: &str =
    "Used instead of the game's arguments when this program starts. Leave empty to use the game's.";
/// Inline editor button.
pub const SAVE_PROGRAM: &str = "Save";
/// Inline editor button.
pub const CANCEL_PROGRAM: &str = "Cancel";

/// "Own arguments: -dx12".
pub fn own_args(args: &str) -> String {
    format!("Own arguments: {args}")
}
