//! Settings (SCREEN_SPECS_2 §13). The labels and help are the design's `SETTINGS` table.

use easyplay_core::model::BasePreset;

use crate::bridge::HandlerState;
use crate::route::SettingsTab;

/// Tab names.
pub fn tab(t: SettingsTab) -> &'static str {
    match t {
        SettingsTab::General => crate::i18n::text("settings.tab.general"),
        SettingsTab::Appearance => crate::i18n::text("settings.tab.appearance"),
        SettingsTab::Protection => crate::i18n::text("settings.tab.protection"),
        SettingsTab::CompatibilityLayers => crate::i18n::text("settings.tab.compatibility_layers"),
        SettingsTab::Integration => crate::i18n::text("settings.tab.integration"),
        SettingsTab::Advanced => crate::i18n::text("settings.tab.advanced"),
        SettingsTab::About => crate::i18n::text("settings.tab.about"),
    }
}

texts! {
    /// Accessible name of the tab bar.
    TABS = "settings.tabs";
}

// ── General ──────────────────────────────────────────────────────────────────────────────
texts! {
    /// Row.
    LANGUAGE = "settings.language";
    /// Option.
    LANG_SYSTEM = "settings.lang_system";
    /// Row.
    CLOSE = "settings.close";
    /// Option.
    CLOSE_QUIT = "settings.close_quit";
    /// Option.
    CLOSE_ASK = "settings.close_ask";
    /// Help.
    CLOSE_HINT = "settings.close_hint";
    /// Row.
    CONFIRM_REMOVE = "settings.confirm_remove";
    /// Row.
    CHECK_MISSING = "settings.check_missing";
    /// Help.
    CHECK_MISSING_HINT = "settings.check_missing_hint";
    /// Section.
    NOTIFICATIONS = "settings.notifications";
    /// Row.
    NOTIFY_EXIT = "settings.notify_exit";
    /// Row.
    NOTIFY_CRASH = "settings.notify_crash";
    /// Section.
    STATISTICS = "settings.statistics";
    /// Row.
    STATS_ON = "settings.stats_on";
    /// Help.
    STATS_ON_HINT = "settings.stats_on_hint";
    /// Row.
    KEEP_REMOVED = "settings.keep_removed";
    /// Row.
    TOOL_SESSIONS = "settings.tool_sessions";
    /// Row.
    WEEK_START = "settings.week_start";
    /// Row.
    MIN_SESSION = "settings.min_session";
    /// Row.
    CLEAR_STATS = "settings.clear_stats";
    /// Help.
    CLEAR_STATS_HINT = "settings.clear_stats_hint";
    /// Button.
    CLEAR = "settings.clear";
    /// Section.
    ARTWORK = "settings.artwork";
    /// Row.
    ARTWORK_ONLINE = "settings.artwork_online";
    /// Option.
    ARTWORK_OFF = "settings.artwork_off";
    /// Option.
    ARTWORK_STEAM = "settings.artwork_steam";
    /// Help.
    ARTWORK_HINT = "settings.artwork_hint";
    /// Row.
    NAME_SEARCH = "settings.name_search";
    /// Help.
    NAME_SEARCH_HINT = "settings.name_search_hint";
}

// ── Appearance ───────────────────────────────────────────────────────────────────────────
texts! {
    /// Row.
    THEME = "settings.theme";
    /// Row.
    ACCENT = "settings.accent";
}
/// The accents' names, in [`crate::theme::palette::Accent::ALL`] order.
#[allow(non_snake_case)]
pub fn ACCENTS() -> [&'static str; 4] {
    [
        crate::i18n::text("settings.accents.tide"),
        crate::i18n::text("settings.accents.ember"),
        crate::i18n::text("settings.accents.iris"),
        crate::i18n::text("settings.accents.moss"),
    ]
}
texts! {
    /// Row.
    UI_SCALE = "settings.ui_scale";
    /// Button.
    RESET = "settings.reset";
    /// Row.
    REDUCE_MOTION = "settings.reduce_motion";
    /// Help.
    REDUCE_MOTION_HINT = "settings.reduce_motion_hint";
    /// Section.
    LIBRARY = "settings.library";
    /// Row.
    LIBRARY_VIEW = "settings.library_view";
    /// Row.
    DENSITY = "settings.density";
    /// Row.
    PLAYTIME_ON_CARDS = "settings.playtime_on_cards";
    /// Option.
    SYSTEM = "settings.system";
    /// Option.
    DARK = "settings.dark";
    /// Option.
    LIGHT = "settings.light";
    /// Option.
    HIGH_CONTRAST = "settings.high_contrast";
    /// Option.
    ON = "settings.on";
    /// Option.
    OFF = "settings.off";
    /// Option.
    COVERS = "settings.covers";
    /// Option.
    LIST = "settings.list";
    /// Option.
    COMPACT = "settings.compact";
    /// Option.
    COMFORTABLE = "settings.comfortable";
    /// Option.
    LARGE = "settings.large";
    /// Option.
    MONDAY = "settings.monday";
    /// Option.
    SUNDAY = "settings.sunday";
}

// ── Protection ───────────────────────────────────────────────────────────────────────────
texts! {
    /// Section.
    NEW_GAMES = "settings.new_games";
    /// Row.
    DEFAULT_PRESET = "settings.default_preset";
}
/// The levels' names, in [`BasePreset::ALL`] order.
#[allow(non_snake_case)]
pub fn PRESETS() -> [&'static str; 3] {
    [
        crate::i18n::text("settings.presets.offline"),
        crate::i18n::text("settings.presets.online"),
        crate::i18n::text("settings.presets.standard"),
    ]
}
/// What a level allows, under "Start with".
pub fn preset_help(p: BasePreset) -> &'static str {
    match p {
        BasePreset::Offline => crate::i18n::text("settings.preset_help.no_internet_it_can"),
        BasePreset::Online => crate::i18n::text("settings.preset_help.internet_allowed_it_can"),
        BasePreset::Standard => {
            crate::i18n::text("settings.preset_help.internet_allowed_with_fewer")
        }
    }
}
texts! {
    /// Row.
    CONTROLLERS_DEFAULT = "settings.controllers_default";
    /// Option.
    CONNECTED_ONLY = "settings.connected_only";
    /// Option.
    ALL_WHEN_ALLOWED = "settings.all_when_allowed";
    /// Section.
    ISOLATION = "settings.isolation";
    /// Help of "Always use a private copy" on a disk that makes instant copies.
    ISOLATION_REFLINK = "settings.isolation_reflink";
    /// Help of "Always use a private copy" on a disk that cannot.
    ISOLATION_SHARED = "settings.isolation_shared";
    /// Row.
    ALWAYS_ISOLATE = "settings.always_isolate";
    /// Section.
    SAFETY = "settings.safety";
    /// Row.
    PREPARE_ONLINE = "settings.prepare_online";
    /// Help.
    PREPARE_ONLINE_HINT = "settings.prepare_online_hint";
    /// Row.
    SECCOMP_DEFAULT = "settings.seccomp_default";
    /// Row.
    WARN_X11 = "settings.warn_x11";
}

// ── Compatibility layers ─────────────────────────────────────────────────────────────────
texts! {
    /// Row.
    DEFAULT_LAYER = "settings.default_layer";
    /// Section.
    INSTALLED = "settings.installed";
}
/// Column heads of the installed layers.
#[allow(non_snake_case)]
pub fn LAYER_COLUMNS() -> [&'static str; 4] {
    [
        crate::i18n::text("settings.layer_columns.layer"),
        crate::i18n::text("settings.layer_columns.from"),
        crate::i18n::text("settings.layer_columns.size"),
        crate::i18n::text("settings.layer_columns.in_use"),
    ]
}
texts! {
    /// Empty.
    NONE_YET = "settings.none_yet";
    /// Button.
    REMOVE = "settings.remove";
    /// Where the Steam Linux Runtime comes from.
    FROM_STEAM = "settings.from_steam";
    /// Button.
    VERIFY = "settings.verify";
    /// Section.
    UPDATES = "settings.updates";
    /// Row.
    CHECK_UPDATES = "settings.check_updates";
    /// Button.
    CHECK_NOW = "settings.check_now";
    /// Row.
    AUTO_UPDATE = "settings.auto_update";
    /// Row.
    INTERVAL = "settings.interval";
    /// Row.
    AUTO_REMOVE = "settings.auto_remove";
    /// Row.
    SEARCH_FOLDERS = "settings.search_folders";
    /// Help.
    SEARCH_FOLDERS_HINT = "settings.search_folders_hint";
    /// Button.
    ADD_SEARCH_FOLDER = "settings.add_search_folder";
}

// ── Integration ──────────────────────────────────────────────────────────────────────────
texts! {
    /// Section.
    FILE_TYPES = "settings.file_types";
    /// Row.
    EXE_FILES = "settings.exe_files";
    /// Button.
    USE_EASYPLAY = "settings.use_easyplay";
    /// Button.
    RESTORE_PREVIOUS = "settings.restore_previous";
    /// Row, while EasyPlay is the default.
    PREVIOUS_HANDLER = "settings.previous_handler";
    /// Help.
    PREVIOUS_HANDLER_HINT = "settings.previous_handler_hint";
    /// Row.
    HANDLE_EXE = "settings.handle_exe";
    /// Option.
    ASK = "settings.ask";
    /// Option.
    YES = "settings.yes";
    /// Option.
    NO = "settings.no";
    /// Row.
    QUICK_WINDOW = "settings.quick_window";
    /// Help.
    QUICK_WINDOW_HINT = "settings.quick_window_hint";
    /// Section.
    CONTROLS = "settings.controls";
    /// Row.
    SINGLE_CLICK = "settings.single_click";
    /// Row.
    GAMEPAD = "settings.gamepad";
    /// Row.
    SINGLE_KEYS = "settings.single_keys";
    /// Help.
    SINGLE_KEYS_HINT = "settings.single_keys_hint";
    /// Row.
    ALL_KEYS = "settings.all_keys";
    /// Button.
    SHOW_ALL = "settings.show_all";
    /// Section.
    MENU_SHORTCUTS = "settings.menu_shortcuts";
    /// Empty.
    NO_SHORTCUTS = "settings.no_shortcuts";
}

// ── Advanced ─────────────────────────────────────────────────────────────────────────────
texts! {
    /// Section.
    DEVELOPER = "settings.developer";
    /// Row.
    DEVELOPER_MODE = "settings.developer_mode";
    /// Help.
    DEVELOPER_HINT = "settings.developer_hint";
    /// Row.
    LOG_LEVEL = "settings.log_level";
}
/// Options, in `LogLevel::ALL` order.
#[allow(non_snake_case)]
pub fn LOG_LEVELS() -> [&'static str; 5] {
    [
        crate::i18n::text("settings.log_levels.errors"),
        crate::i18n::text("settings.log_levels.warnings"),
        crate::i18n::text("settings.log_levels.info"),
        crate::i18n::text("settings.log_levels.debug"),
        crate::i18n::text("settings.log_levels.trace"),
    ]
}
texts! {
    /// Row.
    LOG_CAP = "settings.log_cap";
    /// Row.
    KEEP_LOGS = "settings.keep_logs";
    /// Row.
    KEEP_LOG_DAYS = "settings.keep_log_days";
    /// Row.
    VERBOSE = "settings.verbose";
    /// Row.
    STOP_GRACE = "settings.stop_grace";
    /// Row.
    RENDERER = "settings.renderer";
    /// Help.
    RENDERER_HINT = "settings.renderer_hint";
    /// Option.
    AUTOMATIC = "settings.automatic";
    /// Section.
    GLOBAL_ENV = "settings.global_env";
    /// Placeholder of a new variable's name.
    ENV_NAME = "settings.env_name";
    /// Placeholder of a new variable's value.
    ENV_VALUE = "settings.env_value";
}
/// Between name and value.
pub const ENV_EQUALS: &str = "=";
texts! {
    /// Button.
    ADD = "settings.add";
    /// Section.
    PATHS = "settings.paths";
}
/// Row.
pub const UMU_RUN: &str = "umu-run";
/// Row.
pub const BWRAP: &str = "bwrap";
texts! {
    /// Section.
    MAINTENANCE = "settings.maintenance";
    /// Row.
    DATA_FOLDER = "settings.data_folder";
    /// Help.
    DATA_FOLDER_HINT = "settings.data_folder_hint";
    /// Button.
    OPEN = "settings.open";
    /// Row.
    WELCOME_STEPS = "settings.welcome_steps";
    /// Button.
    SHOW_AGAIN = "settings.show_again";
}

// ── About ────────────────────────────────────────────────────────────────────────────────
texts! {
    /// About's headline.
    PRIVACY = "settings.privacy";
    /// The sentence under it.
    PRIVACY_ENDPOINTS = "settings.privacy_endpoints";
    /// Section.
    LICENCES = "settings.licences";
}
/// The licences: what, and under which licence.
#[allow(non_snake_case)]
pub fn LICENCE_ROWS() -> [(&'static str, &'static str); 4] {
    [
        ("EasyPlay", "GPL-3.0-or-later"),
        (
            crate::i18n::text("settings.licence_rows.inter_instrument_sans_and"),
            "SIL Open Font License 1.1",
        ),
        (
            crate::i18n::text("settings.licence_rows.phosphor_icons"),
            "MIT",
        ),
        (
            crate::i18n::text("settings.licence_rows.third_party_crates"),
            crate::i18n::text("settings.licence_rows.see_the_licence_list"),
        ),
    ]
}

/// "Used by every game set to Default: 17 games."
pub fn default_used(n: Option<u32>) -> String {
    match n {
        Some(n) => crate::i18n::plural::counted("settings.default_used_count", u64::from(n)),
        None => crate::i18n::text("settings.default_used").to_owned(),
    }
}
/// "GE-Proton (latest)".
pub fn channel_latest(name: &str) -> String {
    rust_i18n::t!("settings.channel_latest", name = name).into_owned()
}
/// "17 games", or "Not used".
pub fn used_by(n: u32) -> String {
    match n {
        0 => crate::i18n::text("settings.not_used").to_owned(),
        n => crate::i18n::plural::counted("settings.used_by_count", u64::from(n)),
    }
}
/// "Verified Tuesday".
pub fn verified(when: &str) -> String {
    rust_i18n::t!("settings.verified", when = when).into_owned()
}
/// "Last checked today."
pub fn last_checked(when: &str) -> String {
    rust_i18n::t!("settings.last_checked", when = when).into_owned()
}
/// "Downloading GE-Proton 10-25 · 120 MB".
pub fn downloading(what: &str, bytes: &str) -> String {
    rust_i18n::t!("settings.downloading", what = what, bytes = bytes).into_owned()
}
/// "Layers use 3.6 GB on disk."
pub fn disk_total(size: &str) -> String {
    rust_i18n::t!("settings.disk_total", size = size).into_owned()
}
/// "Remove /opt/proton".
pub fn remove_folder(p: &str) -> String {
    rust_i18n::t!("settings.remove_folder", p = p).into_owned()
}
/// "Remove DXVK_HUD".
pub fn remove_var(name: &str) -> String {
    rust_i18n::t!("settings.remove_var", name = name).into_owned()
}
/// "Path to umu-run".
pub fn path_to(program: &str) -> String {
    rust_i18n::t!("settings.path_to", program = program).into_owned()
}
/// The help of ".exe and .msi files": who opens them.
pub fn handler(h: &HandlerState) -> String {
    match h {
        HandlerState::EasyPlay => crate::i18n::text("settings.handler.easyplay").to_owned(),
        HandlerState::Other(n) => rust_i18n::t!("settings.handler.named", name = n).into_owned(),
        HandlerState::None => crate::i18n::text("settings.handler.none").to_owned(),
        HandlerState::Unknown => crate::i18n::text("settings.handler.unknown").to_owned(),
    }
}
/// "Version 0.1.0".
pub fn version(v: &str) -> String {
    rust_i18n::t!("settings.version", v = v).into_owned()
}
/// "30 s", "1 min", "2 min".
pub fn seconds(n: i64) -> String {
    if n >= 60 && n % 60 == 0 {
        rust_i18n::t!("settings.minutes_short", n = n / 60).into_owned()
    } else {
        rust_i18n::t!("settings.seconds_short", n = n).into_owned()
    }
}
/// "24 h", "3 days", "7 days".
pub fn hours(n: i64) -> String {
    if n >= 48 && n % 24 == 0 {
        days(n / 24)
    } else {
        rust_i18n::t!("settings.hours_short", n = n).into_owned()
    }
}
/// "32 MB".
pub fn megabytes(n: i64) -> String {
    rust_i18n::t!("settings.megabytes", n = n).into_owned()
}
/// "10".
pub fn count(n: i64) -> String {
    n.to_string()
}
/// "30 days".
pub fn days(n: i64) -> String {
    crate::i18n::plural::counted("settings.days", n as u64)
}

/// "125 %".
pub fn percent(v: f64) -> String {
    format!("{:.0} %", v * 100.0)
}

/// "DXVK_HUD=fps".
pub fn env_pair(name: &str, value: &str) -> String {
    rust_i18n::t!("settings.env_pair", name = name, value = value).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_values_read_as_the_design_writes_them() {
        assert_eq!(seconds(0), "0 s");
        assert_eq!(seconds(30), "30 s");
        assert_eq!(seconds(60), "1 min");
        assert_eq!(seconds(300), "5 min");
        assert_eq!(hours(24), "24 h");
        assert_eq!(hours(168), "7 days");
        assert_eq!(used_by(17), "17 games");
        assert_eq!(used_by(0), "Not used");
        assert_eq!(
            default_used(Some(17)),
            "Used by every game set to Default: 17 games."
        );
        assert_eq!(last_checked("today"), "Last checked today.");
    }
}
