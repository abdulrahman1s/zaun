//! The game page (SCREEN_SPECS §4, SCREEN_SPECS_2 §8–11).

use std::path::Path;

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Availability, BasePreset, Permissions, SandboxPolicy};

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
/// On a cover that has no picture yet.
pub const ADD_COVER: &str = "Add cover";
/// Beside the protection, while the game has no tags.
pub const ADD_TAG: &str = "Add tag";
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

/// Label beside Play.
pub const TIME_PLAYED: &str = "Time played";
/// Label beside Play.
pub const LAST_7_DAYS: &str = "Last 7 days";

/// "Last 7 days: 1 hour 12 minutes".
pub fn week_name(ms: u64) -> String {
    format!("{LAST_7_DAYS}: {}", super::fmt::duration_spoken(ms))
}

/// "Last played: Yesterday".
pub fn fact_name(label: &str, value: &str) -> String {
    format!("{label}: {value}")
}

/// Section: what the game's files tell (ADR-0028).
pub const WHAT_IT_IS: &str = "What it is";
/// Fact.
pub const ENGINE: &str = "Engine";
/// Fact: the main program.
pub const PROGRAM: &str = "Program";
/// Fact: the company in the program's version resource.
pub const MADE_BY: &str = "Made by";
/// Fact.
pub const ANTI_CHEAT: &str = "Anti-cheat";
/// After the anti-cheat systems found.
pub const ANTI_CHEAT_RISK: &str = "often refuses to run under Proton or in a sandbox";
/// Fact: Steam and other stores' services.
pub const STORE_SERVICES: &str = "Online services";
/// A finding.
pub const USES_STEAM_API: &str = "Uses Steam's services";
/// A finding.
pub const NEEDS_STEAM: &str = "Needs Steam running to start (Steam DRM)";
/// A finding.
pub const STORE_SDK: &str = "Uses Epic or GOG online services";
/// Fact: Proton's per-game fixes.
pub const GAME_FIXES: &str = "Game fixes";
/// When the layer applies none.
pub const NO_FIXES_HERE: &str = "this compatibility layer applies none";

/// The game Proton's fixes are for (a title, or a umu-database ID).
pub fn fixes_for(known: &str) -> String {
    format!("Proton applies the fixes for {known}")
}

/// Findings joined on one line.
pub fn joined(parts: &[String]) -> String {
    parts.join(" · ")
}

/// Who made the game, and the product's name when it is not the game's.
pub fn made_by(company: &str, product: Option<&str>) -> String {
    match product {
        Some(p) => format!("{company} · {p}"),
        None => company.to_owned(),
    }
}

/// The anti-cheat systems found, and what they mean here.
pub fn anti_cheat(found: &[easyplay_core::identity::AntiCheat]) -> String {
    use easyplay_core::identity::AntiCheat as A;
    let names: Vec<&str> = found
        .iter()
        .map(|a| match a {
            A::EasyAntiCheat => "Easy Anti-Cheat",
            A::BattlEye => "BattlEye",
        })
        .collect();
    format!("{}: {ANTI_CHEAT_RISK}", names.join(", "))
}

/// Proton's fixes for the game, under a layer that applies none.
pub fn fixes_not_applied(known: &str) -> String {
    format!("Fixes for {known}: {NO_FIXES_HERE}")
}

/// What an engine is called, with its variant.
pub fn engine_name(g: easyplay_core::identity::EngineGuess) -> String {
    use easyplay_core::identity::{Engine as E, Variant as V};
    let name = match g.engine {
        E::Unity => "Unity",
        E::Unreal => "Unreal Engine",
        E::Godot => "Godot",
        E::GameMaker => "GameMaker",
        E::RenPy => "Ren'Py",
        E::RpgMaker => "RPG Maker",
        E::Source => "Source",
        E::Source2 => "Source 2",
        E::CryEngine => "CryEngine",
        E::Fna => "FNA",
        E::MonoGame => "MonoGame",
        E::Xna => "XNA",
        E::Love => "LÖVE",
        E::Cocos => "Cocos2d-x",
        E::Electron => "Electron",
        E::NwJs => "NW.js",
    };
    match g.variant {
        None => name.to_owned(),
        Some(V::Il2Cpp) => format!("{name} (IL2CPP)"),
        Some(V::Mono) => format!("{name} (Mono)"),
        Some(V::Xp) => format!("{name} XP"),
        Some(V::Vx) => format!("{name} VX"),
        Some(V::VxAce) => format!("{name} VX Ace"),
        Some(V::Mv) => format!("{name} MV"),
        Some(V::Mz) => format!("{name} MZ"),
    }
}

/// What kind of program the main program is, and its version.
pub fn program_kind(pe: &easyplay_core::model::PeInfo) -> Option<String> {
    use easyplay_core::model::PeMachine as M;
    let kind = if pe.is_dotnet {
        ".NET program"
    } else {
        match pe.machine {
            M::X86_64 => "64-bit Windows program",
            M::X86 => "32-bit Windows program",
            M::Arm64 => "ARM64 Windows program",
            _ => return pe.file_version.as_ref().map(|v| format!("version {v}")),
        }
    };
    Some(match &pe.file_version {
        Some(v) => format!("{kind} · version {v}"),
        None => kind.to_owned(),
    })
}

/// Section: protection, compatibility, launch options.
pub const RUNS: &str = "How it runs";
/// Section: the folders.
pub const LIVES: &str = "Where it lives";
/// The list of access statements.
pub const REACH: &str = "What this game can reach";
/// Row.
pub const GAME_FOLDER: &str = "Game folder";
/// Row, for a game that is a single file.
pub const PROGRAM_FILE: &str = "Program";
/// Row.
pub const SAVE_DATA: &str = "Save data";
/// Row.
pub const OTHER_PROGRAMS: &str = "Other programs";
/// Tooltip of a folder row.
pub const OPEN_IN_FILES: &str = "Open in the file manager";
/// Value.
pub const PRIVATE_STORAGE: &str = "Private storage";
/// Value.
pub const YOUR_FOLDER: &str = "A folder you chose";
/// Section.
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

/// "Protection: Offline", "Game folder: Hollow Knight, in Games".
pub fn row_name(title: &str, value: &str, detail: Option<&str>) -> String {
    match detail {
        Some(d) => format!("{title}: {value}, {d}"),
        None => format!("{title}: {value}"),
    }
}

/// "and 3 more".
pub fn more_statements(n: usize) -> String {
    format!("and {n} more")
}

/// "2 more…".
pub fn more_programs(n: usize) -> String {
    format!("{n} more…")
}

/// "1 variable".
pub fn variables(n: usize) -> String {
    format!("{n} {}", super::plural(n as u64, "variable", "variables"))
}

/// "-windowed · 1 variable": what a game's launch options add, `None` when nothing.
pub fn launch_summary(args: &str, variables_set: usize) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if !args.is_empty() {
        parts.push(args.to_owned());
    }
    if variables_set > 0 {
        parts.push(variables(variables_set));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// "Added 16 Oct", "Added yesterday" (`date` as written inside a sentence).
pub fn added(date: &str) -> String {
    format!("Added {date}")
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

/// Overview row.
pub const PROTECTION: &str = "Protection";
/// Heading and radio group of the Protection tab (the tab itself is called Protection).
pub const PROTECTION_LEVEL: &str = "Protection level";
/// Link.
pub const RESET_TO_PRESET: &str = "Reset to preset";
/// Inline confirm button.
pub const REPLACE: &str = "Replace";
/// Banner while running.
pub const APPLIES_NEXT_TIME: &str = "Changes apply the next time you start the game.";
/// Policy of a newer version.
pub const POLICY_TOO_NEW: &str =
    "These settings were saved by a newer version of EasyPlay and can't be shown here.";
/// On a row while it differs from what is saved.
pub const CHANGED_MARK: &str = "changed";
/// Permission.
pub const INTERNET_ON: &str = "Internet and local network";
/// List of permissions, and a section of the setup sheet.
pub const FILES: &str = "Files and folders";
/// Degree of access.
pub const CAN_READ: &str = "Can read";
/// Degree of access.
pub const CAN_READ_CHANGE: &str = "Can read and change";
/// Degree of access to the game folder.
pub const CHANGES_KEPT_APART: &str = "Changes kept separately";
/// Access (setup sheet).
pub const CAN_CHANGE: &str = "Can change";
/// Row.
pub const ADD_FOLDER: &str = "Add folder…";
/// Drop hint.
pub const DROP_FOLDER: &str = "Drop a folder here to share it";
/// Permission.
pub const REMOVABLE: &str = "Removable drives";
/// Note.
pub const WRITABLE_WITH_INTERNET: &str =
    "With internet access, a program can upload or alter these files.";
/// Option.
pub const CHOOSE_FOLDER: &str = "A folder I choose…";
/// List of permissions.
pub const INTERNET_AND_DEVICES: &str = "Internet and devices";
/// Section of the setup sheet.
pub const DEVICES: &str = "Devices";
/// Permission.
pub const SOUND: &str = "Sound";
/// Permission.
pub const MICROPHONE: &str = "Microphone";
/// Under the microphone permission while it is off, on a computer that cannot keep it out
/// of the sound server (ADR-0026).
pub const MICROPHONE_INSEPARABLE: &str =
    "This computer can't give sound without the microphone: with it off, the game is silent";
/// Toggle (setup sheet).
pub const CONTROLLERS: &str = "Controllers";
/// Permission.
pub const GAME_CONTROLLERS: &str = "Game controllers";
/// Degree of the controllers permission.
pub const CONNECTED_ONLY: &str = "Connected when the game starts";
/// Degree of the controllers permission.
pub const ALL_INPUT: &str = "All input devices";
/// Disabled reason.
pub const ALL_INPUT_UNAVAILABLE: &str =
    "All input devices is unavailable because your account can read keyboards directly.";
/// Permission.
pub const CAMERA: &str = "Camera";
/// Row: where the game draws.
pub const SCREEN: &str = "Screen";
/// X11 notice.
pub const X11_NOTICE: &str = "Shows windows through X11 — other X11 apps are not isolated from it.";
/// Setup sheet.
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

/// A permission and its degree: "Music: Can read".
pub fn permission(title: &str, value: &str) -> String {
    format!("{title}: {value}")
}

/// "Hollow Knight in Games".
pub fn place(name: &str, parent: Option<&str>) -> String {
    match parent {
        Some(parent) => format!("{name} {parent}"),
        None => name.to_owned(),
    }
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
    let advanced = Permissions::COMPUTER_NAME
        | Permissions::MACHINE_ID
        | Permissions::HARDWARE_IDS
        | Permissions::UNFILTERED_SYSCALLS
        | Permissions::DIRECT_SOUND;
    let flags = (a.permissions() ^ b.permissions()) & advanced;
    flags.bits().count_ones() as usize
        + usize::from(a.hardening.runtime_isolation != b.hardening.runtime_isolation)
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
/// The size could not be measured.
pub const SIZE_UNKNOWN: &str = "Size unknown";
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

/// Placeholder of a variable's name field.
pub const FIND_PROTON_OPTION: &str = "Type to find an option";
/// Accessible name of the list of suggested options.
pub const PROTON_OPTIONS: &str = "Proton options";

/// Under an option the game's compatibility layer does not read.
pub fn not_read_by(summary: &str, layer: &str) -> String {
    format!("{summary}. Only GE-Proton and CachyOS builds read it, not {layer}")
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
