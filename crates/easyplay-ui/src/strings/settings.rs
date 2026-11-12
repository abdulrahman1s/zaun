//! Settings (SCREEN_SPECS_2 §13).

use crate::bridge::HandlerState;
use crate::route::SettingsTab;

/// Tab names.
pub fn tab(t: SettingsTab) -> &'static str {
    match t {
        SettingsTab::General => "General",
        SettingsTab::Appearance => "Appearance",
        SettingsTab::Protection => "Protection",
        SettingsTab::CompatibilityLayers => "Compatibility layers",
        SettingsTab::Integration => "Integration",
        SettingsTab::Advanced => "Advanced",
        SettingsTab::About => "About",
    }
}

/// Section.
pub const GENERAL: &str = "General";
/// Row.
pub const LANGUAGE: &str = "Language";
/// Option.
pub const LANG_SYSTEM: &str = "Same as system";
/// Row.
pub const CLOSE: &str = "When I close the window";
/// Option.
pub const CLOSE_QUIT: &str = "Quit";
/// Option.
pub const CLOSE_ASK: &str = "Ask if games are running";
/// Hint.
pub const CLOSE_HINT: &str = "Games keep running and are tracked without the window.";
/// Toggle.
pub const CONFIRM_REMOVE: &str = "Ask before removing a game";
/// Toggle.
pub const CHECK_MISSING: &str = "Check for missing games when EasyPlay comes to the front";
/// Hint.
pub const CHECK_MISSING_HINT: &str = "Notices moved games and disconnected drives quickly.";
/// Section.
pub const NOTIFICATIONS: &str = "Notifications";
/// Toggle.
pub const NOTIFY_EXIT: &str = "Notify me when a game ends";
/// Toggle.
pub const NOTIFY_CRASH: &str = "Notify me when a game crashes";
/// Section.
pub const STATISTICS: &str = "Statistics";
/// Toggle.
pub const STATS_ON: &str = "Record playtime";
/// Hint.
pub const STATS_ON_HINT: &str = "Kept on this computer only.";
/// Toggle.
pub const KEEP_REMOVED: &str = "Keep the history of removed games";
/// Toggle.
pub const TOOL_SESSIONS: &str = "Show tool and installer sessions in history";
/// Row.
pub const WEEK_START: &str = "Weeks start on";
/// Row.
pub const MIN_SESSION: &str = "Count sessions longer than";
/// Section.
pub const ARTWORK: &str = "Artwork";
/// Row.
pub const ARTWORK_ONLINE: &str = "Official artwork";
/// Option.
pub const ARTWORK_OFF: &str = "Off";
/// Option.
pub const ARTWORK_STEAM: &str = "From Steam's image server";
/// Hint.
pub const ARTWORK_HINT: &str = "When on, EasyPlay sends a game's Steam app ID to Steam's image server to download its cover. Nothing else is sent.";
/// Toggle.
pub const NAME_SEARCH: &str = "Look up app IDs by game name";
/// Hint.
pub const NAME_SEARCH_HINT: &str = "Also sends the game's name to Steam's store search.";
/// Button.
pub const CLEAR_STATS: &str = "Clear all statistics…";
/// Section.
pub const APPEARANCE: &str = "Appearance";
/// Row.
pub const THEME: &str = "Theme";
/// Row.
pub const ACCENT: &str = "Accent";
/// Row.
pub const UI_SCALE: &str = "Interface size";
/// Button.
pub const RESET: &str = "Reset";
/// Row.
pub const REDUCE_MOTION: &str = "Reduce motion";
/// Row.
pub const LIBRARY_VIEW: &str = "Library view";
/// Row.
pub const DENSITY: &str = "Cover size";
/// Toggle.
pub const PLAYTIME_ON_CARDS: &str = "Show playtime on cards";
/// Row with a reset button for widths set by dragging list headers.
pub const LIST_COLUMNS: &str = "List column widths";
/// Option.
pub const SYSTEM: &str = "System";
/// Option.
pub const DARK: &str = "Dark";
/// Option.
pub const LIGHT: &str = "Light";
/// Option.
pub const HIGH_CONTRAST: &str = "High contrast";
/// Option.
pub const ON: &str = "On";
/// Option.
pub const OFF: &str = "Off";
/// Option.
pub const GRID: &str = "Grid";
/// Option.
pub const LIST: &str = "List";
/// Option.
pub const COMPACT: &str = "Compact";
/// Option.
pub const COMFORTABLE: &str = "Comfortable";
/// Option.
pub const LARGE: &str = "Large";
/// Option.
pub const MONDAY: &str = "Monday";
/// Option.
pub const SUNDAY: &str = "Sunday";
/// Section.
pub const PROTECTION: &str = "Protection";
/// Row.
pub const DEFAULT_PRESET: &str = "New games start with";
/// Row.
pub const CONTROLLERS_DEFAULT: &str = "Controllers by default";
/// Option.
pub const CONNECTED_ONLY: &str = "Connected only";
/// Option.
pub const ALL_WHEN_ALLOWED: &str = "All input devices when allowed";
/// Section.
pub const ISOLATION: &str = "Compatibility layer copies";
/// Explanation.
pub const ISOLATION_REFLINK: &str =
    "Automatic — your disk supports instant copies, so each game gets its own copy of the layer.";
/// Explanation.
pub const ISOLATION_SHARED: &str =
    "Automatic — shared mode: games share the layer and its files are checked after each game.";
/// Toggle.
pub const ALWAYS_ISOLATE: &str = "Always use a private copy";
/// Hint.
pub const ALWAYS_ISOLATE_HINT: &str = "Slower starts on some disks.";
/// Toggle.
pub const PREPARE_ONLINE: &str = "Allow internet while preparing a brand-new game";
/// Hint.
pub const PREPARE_ONLINE_HINT: &str =
    "Some games need to download runtime components the first time.";
/// Toggle.
pub const SECCOMP_DEFAULT: &str = "Filter risky system calls";
/// Toggle.
pub const WARN_X11: &str = "Warn me when my desktop uses X11";
/// Section.
pub const LAYERS: &str = "Compatibility layers";
/// Row.
pub const DEFAULT_LAYER: &str = "Default layer";
/// Section.
pub const INSTALLED: &str = "Installed";
/// Empty.
pub const NONE_YET: &str = "None yet. One will be downloaded when you first start a game.";
/// Button.
pub const REMOVE: &str = "Remove";
/// Section.
pub const RUNTIME: &str = "Steam Linux Runtime";
/// Button.
pub const VERIFY: &str = "Verify now";
/// Section.
pub const UPDATES: &str = "Updates";
/// Button.
pub const CHECK_UPDATES: &str = "Check for updates";
/// Toggle.
pub const AUTO_UPDATE: &str = "Update layers automatically";
/// Row.
pub const INTERVAL: &str = "Check every";
/// Toggle.
pub const AUTO_REMOVE: &str = "Remove layers no game uses";
/// Section.
pub const SEARCH_FOLDERS: &str = "Extra places to look for layers";
/// Button.
pub const ADD_SEARCH_FOLDER: &str = "Add folder…";
/// Section.
pub const FILE_TYPES: &str = "Opening Windows programs";
/// Row.
pub const EXE_FILES: &str = ".exe and .msi files:";
/// Button.
pub const USE_EASYPLAY: &str = "Use EasyPlay";
/// Button.
pub const RESTORE_PREVIOUS: &str = "Restore previous";
/// Row.
pub const HANDLE_EXE: &str = "When EasyPlay isn't the default";
/// Option.
pub const ASK: &str = "Ask";
/// Option.
pub const YES: &str = "Make it the default";
/// Option.
pub const NO: &str = "Don't ask";
/// Toggle.
pub const QUICK_WINDOW: &str = "Show a small window when starting from the file manager";
/// Hint.
pub const QUICK_WINDOW_HINT: &str = "Otherwise the game starts without any window from EasyPlay.";
/// Toggle.
pub const SINGLE_CLICK: &str = "Play with a single click on a cover";
/// Toggle.
pub const GAMEPAD: &str = "Use a controller to move around EasyPlay";
/// Toggle.
pub const SINGLE_KEYS: &str = "Single-key shortcuts (F, T, H, /)";
/// Hint.
pub const SINGLE_KEYS_HINT: &str =
    "Turn off if they get in the way of screen readers or other tools.";
/// Section.
pub const MENU_SHORTCUTS: &str = "Applications menu shortcuts";
/// Empty.
pub const NO_SHORTCUTS: &str = "No games are in the applications menu.";
/// Section.
pub const DEVELOPER: &str = "Developer Mode";
/// Toggle.
pub const DEVELOPER_MODE: &str = "Use Developer Mode";
/// Hint.
pub const DEVELOPER_HINT: &str = "Shows logs, the launch inspector and technical options.";
/// Row.
pub const LOG_LEVEL: &str = "Log detail";
/// Row.
pub const LOG_CAP: &str = "Largest session log";
/// Row.
pub const KEEP_LOGS: &str = "Session logs to keep per game";
/// Row.
pub const KEEP_LOG_DAYS: &str = "Delete session logs older than";
/// Toggle.
pub const VERBOSE: &str = "Detailed launch log";
/// Row.
pub const STOP_GRACE: &str = "When stopping a game, wait";
/// Row.
pub const RENDERER: &str = "Drawing method";
/// Option.
pub const AUTOMATIC: &str = "Automatic";
/// Note.
pub const RESTART: &str = "Restart EasyPlay to apply.";
/// Section.
pub const GLOBAL_ENV: &str = "Environment variables for every game";
/// Section.
pub const PATHS: &str = "Program locations";
/// Field.
pub const UMU_RUN: &str = "umu-run";
/// Field.
pub const BWRAP: &str = "bwrap";
/// Section.
pub const MAINTENANCE: &str = "Maintenance";
/// Button.
pub const CHECK_SYSTEM: &str = "Check my system";
/// Button.
pub const OPEN_DATA: &str = "Open data folder";
/// Button.
pub const RUN_WELCOME: &str = "Show the welcome steps again";
/// About.
pub const PRIVACY: &str = "EasyPlay sends nothing about you or your games anywhere.";
/// About.
pub const PRIVACY_ENDPOINTS: &str = "It connects only to download compatibility layers and, if you turn it on, official artwork. The full list of addresses is in the privacy notes.";
/// About.
pub const LICENCES: &str = "Licences";
/// About.
pub const LICENCE_LINES: [&str; 4] = [
    "EasyPlay: GPL-3.0-or-later",
    "Inter and JetBrains Mono: SIL Open Font License 1.1",
    "Phosphor Icons: MIT",
    "Third-party crates: see the licence list shipped with EasyPlay",
];

/// "Affects 12 games".
pub fn affects(n: u32) -> String {
    format!(
        "Affects {n} {}",
        super::plural(u64::from(n), "game", "games")
    )
}
/// "GE-Proton (latest)".
pub fn channel_latest(name: &str) -> String {
    format!("{name} (latest)")
}
/// "Used by 3 games".
pub fn used_by(n: u32) -> String {
    match n {
        0 => "Not used".to_owned(),
        n => format!(
            "Used by {n} {}",
            super::plural(u64::from(n), "game", "games")
        ),
    }
}
/// "Verified yesterday".
pub fn verified(when: &str) -> String {
    format!("Verified {when}")
}
/// "Last checked today".
pub fn last_checked(when: &str) -> String {
    format!("Last checked {when}")
}
/// "Downloading GE-Proton 10-25 · 120 MB".
pub fn downloading(what: &str, bytes: &str) -> String {
    format!("Downloading {what} · {bytes}")
}
/// "Layers use 3.6 GB".
pub fn disk_total(size: &str) -> String {
    format!("Layers use {size} on disk.")
}
/// "Remove /opt/proton".
pub fn remove_folder(p: &str) -> String {
    format!("Remove {p}")
}
/// ".exe files open with Wine".
pub fn handler(h: &HandlerState) -> String {
    match h {
        HandlerState::EasyPlay => "EasyPlay".to_owned(),
        HandlerState::Other(n) => n.clone(),
        HandlerState::None => "Nothing".to_owned(),
        HandlerState::Unknown => "Checking…".to_owned(),
    }
}
/// "Version 0.1.0".
pub fn version(v: &str) -> String {
    format!("Version {v}")
}
/// "30 s".
pub fn seconds(n: i64) -> String {
    format!("{n} s")
}
/// "24 h".
pub fn hours(n: i64) -> String {
    format!("{n} h")
}
/// "32 MB".
pub fn megabytes(n: i64) -> String {
    format!("{n} MB")
}
/// "10".
pub fn count(n: i64) -> String {
    n.to_string()
}
/// "30 days".
pub fn days(n: i64) -> String {
    format!("{n} {}", super::plural(n as u64, "day", "days"))
}

/// "125 %".
pub fn percent(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

/// "DXVK_HUD=fps".
pub fn env_pair(name: &str, value: &str) -> String {
    format!("{name}={value}")
}

/// "EasyPlay · 1.1 GB · Used by 21 games" (narrow layer rows).
pub fn layer_details(origin: &str, size: &str, used: &str) -> String {
    format!("{origin} · {size} · {used}")
}
