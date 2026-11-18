//! `Preferences`: every key of CONFIGURATION §2 with its default.
//!
//! Field order is the order of the documented file; serialising [`Preferences::default`]
//! reproduces it (modulo comments). Enumerations use the documented kebab-case words.
//! Keys this build does not know are kept opaquely ([`Preferences::unknown`]) and written
//! back on save.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::custom;
use crate::consts::CONFIG_SCHEMA_VERSION;
use crate::ids::RuntimeName;
use crate::model::{BasePreset, EnvVar};

str_enum!(
    /// `general.close_behavior`.
    pub CloseBehavior { Quit = "quit", AskIfRunning = "ask-if-running" } default Quit
);
str_enum!(
    /// `appearance.theme`.
    pub Theme { System = "system", Dark = "dark", Light = "light", HighContrast = "high-contrast" } default System
);
str_enum!(
    /// `appearance.accent`.
    pub Accent { Tide = "tide", Ember = "ember", Iris = "iris", Moss = "moss" } default Tide
);
str_enum!(
    /// `appearance.reduce_motion`.
    pub ReduceMotion { System = "system", On = "on", Off = "off" } default System
);
str_enum!(
    /// `appearance.library_view`.
    pub LibraryView { Grid = "grid", List = "list" } default Grid
);
str_enum!(
    /// `appearance.grid_density`.
    pub GridDensity { Compact = "compact", Comfortable = "comfortable", Large = "large" } default Comfortable
);
str_enum!(
    /// `artwork.online`: where artwork may be fetched from (ADR-0018).
    pub ArtworkOnline { Off = "off", Steam = "steam" } default Off
);
str_enum!(
    /// `protection.controller_access_default`.
    pub ControllerDefault { Detected = "detected", AllWhenSafe = "all-when-safe" } default Detected
);
str_enum!(
    /// `statistics.week_starts_on`.
    pub WeekStart { System = "system", Monday = "monday", Sunday = "sunday" } default System
);
str_enum!(
    /// `logging.level`.
    pub LogLevel { Error = "error", Warn = "warn", Info = "info", Debug = "debug", Trace = "trace" } default Info
);
str_enum!(
    /// `integration.handle_exe`.
    pub HandleExe { Ask = "ask", Yes = "yes", No = "no" } default Ask
);
str_enum!(
    /// `advanced.renderer`.
    pub RendererPref { Auto = "auto", Wgpu = "wgpu", Glow = "glow" } default Auto
);
str_enum!(
    /// `chat.emoji`: how emoji are drawn (Twemoji everywhere, or the system's emoji font).
    pub ChatEmoji { Twemoji = "twemoji", System = "system" } default Twemoji
);
str_enum!(
    /// `chat.send_with`: the key that sends a message (the other one breaks the line).
    pub ChatSendWith { Enter = "enter", CtrlEnter = "ctrl_enter" } default Enter
);
str_enum!(
    /// `chat.share_quality`: the picture of a shared game: its height at most, never more
    /// than the game draws (`source`: as in the game). 1440 came last, so the order of the
    /// first three is unchanged (ADR-0036).
    pub ShareQuality { P720 = "720", P1080 = "1080", Source = "source", P1440 = "1440" } default P1080
);
str_enum!(
    /// `chat.share_fps`: frames a second of a shared game at most (never more than the game
    /// draws; ADR-0036).
    pub ShareFps { F30 = "30", F60 = "60" } default F60
);
str_enum!(
    /// `chat.overlay_corner`: where the in-game overlay's call widget sits (ADR-0037).
    pub OverlayCorner { TopLeft = "top_left", TopRight = "top_right", BottomLeft = "bottom_left", BottomRight = "bottom_right" } default TopLeft
);

/// `runtime.default`: `umu-proton`, `ge-proton`, `pinned:<name>`, `external:<path>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum RuntimeDefault {
    /// Newest UMU-Proton.
    #[default]
    UmuProton,
    /// Newest GE-Proton.
    GeProton,
    /// An exact build in the store.
    Pinned(RuntimeName),
    /// An exact directory (absolute).
    External(PathBuf),
}

impl fmt::Display for RuntimeDefault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeDefault::UmuProton => f.write_str("umu-proton"),
            RuntimeDefault::GeProton => f.write_str("ge-proton"),
            RuntimeDefault::Pinned(n) => write!(f, "pinned:{n}"),
            RuntimeDefault::External(p) => write!(f, "external:{}", p.display()),
        }
    }
}

impl FromStr for RuntimeDefault {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let lower = s.to_ascii_lowercase();
        if lower == "umu-proton" {
            return Ok(RuntimeDefault::UmuProton);
        }
        if lower == "ge-proton" {
            return Ok(RuntimeDefault::GeProton);
        }
        if lower.starts_with("pinned:") {
            return RuntimeName::new(&s["pinned:".len()..])
                .map(RuntimeDefault::Pinned)
                .map_err(|e| e.to_string());
        }
        if lower.starts_with("external:") {
            let p = PathBuf::from(&s["external:".len()..]);
            return if p.is_absolute() {
                Ok(RuntimeDefault::External(p))
            } else {
                Err("external path must be absolute".into())
            };
        }
        Err(format!("unknown compatibility layer {s:?}"))
    }
}

impl TryFrom<String> for RuntimeDefault {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

impl From<RuntimeDefault> for String {
    fn from(r: RuntimeDefault) -> String {
        r.to_string()
    }
}

settings! {
    /// `[general]`.
    General "general" {
        /// `"system"` or a BCP-47 tag.
        language: String = "system".into() => custom(custom::language),
        close_behavior: CloseBehavior = CloseBehavior::Quit => word,
        confirm_remove: bool = true => bool,
        show_tool_sessions_in_history: bool = false => bool,
        /// Re-check missing games when the window gains focus.
        check_missing_on_focus: bool = true => bool,
    }

    /// `[appearance]`.
    Appearance "appearance" {
        theme: Theme = Theme::System => word,
        accent: Accent = Accent::Tide => word,
        /// `0.75 ..= 2.0`, multiplies the system scale.
        ui_scale: f64 = 1.0 => float(0.75, 2.0),
        reduce_motion: ReduceMotion = ReduceMotion::System => word,
        /// Initial library view.
        library_view: LibraryView = LibraryView::Grid => word,
        grid_density: GridDensity = GridDensity::Comfortable => word,
        show_playtime_on_cards: bool = true => bool,
        /// Widths of the library list's optional columns (absent = the default width).
        list_column_widths: Vec<ColumnWidth> = Vec::new() => custom(custom::column_widths),
    }

    /// `[artwork]` (ADR-0018).
    ArtworkPrefs "artwork" {
        /// `"off"` (never contact a server) or `"steam"` (fetch from the Steam CDN).
        online: ArtworkOnline = ArtworkOnline::Off => word,
        /// With `online = "steam"` and no known app ID, the game's display name may be sent
        /// to Steam's store search.
        name_search: bool = true => bool,
    }

    /// `[steam]` (ADR-0031).
    SteamPrefs "steam" {
        /// "Show my Steam games": list the games installed with Steam on this computer.
        /// While off, nothing of Steam's is read.
        show_games: bool = false => bool,
    }

    /// `[chat]` (ADR-0033).
    ChatPrefs "chat" {
        /// The chat engine starts with Zaun and reaches the relays. While off, nothing
        /// is sent anywhere.
        enabled: bool = false => bool,
        /// The name friends see, used when the chat profile is made (the first start);
        /// later changes go through the profile.
        name: String = String::new() => string,
        /// `smp://…` relays and `xftp://…` file servers; empty = the preset public ones.
        relays: Vec<String> = Vec::new() => custom(custom::relays),
        /// A desktop notification for a new message while no Zaun window has focus.
        notify: bool = true => bool,
        /// The notification includes the message's text (otherwise only who wrote).
        notify_preview: bool = false => bool,
        /// Friends see the game's name while it runs (hidden games never).
        show_playing: bool = true => bool,
        emoji: ChatEmoji = ChatEmoji::Twemoji => word,
        /// Enter or Ctrl+Enter sends.
        send_with: ChatSendWith = ChatSendWith::Enter => word,
        /// GIFs play by themselves (otherwise on click).
        gif_autoplay: bool = true => bool,
        /// The typed search words go to the GIF service (Giphy, through a relay).
        gif_search: bool = true => bool,
        /// Links the user sends get a preview (the sender fetches the page).
        link_previews: bool = false => bool,
        /// Before a link opens, Google Safe Browsing is asked about its site (which tells
        /// Google the site's name).
        check_links: bool = false => bool,
        /// The Giphy API key (`crate::gifs::key`); by default Signal's public key, empty =
        /// no GIF search. Never logged; `<set>` in reports.
        gif_key: String = crate::consts::GIF_DEFAULT_KEY.to_owned() => string,
        /// The HTTP CONNECT proxy GIF search goes through (`http://host:port`); by default
        /// Signal's content proxy, empty = directly to Giphy.
        gif_proxy: String = crate::consts::GIF_DEFAULT_PROXY.to_owned() => string,
        /// The microphone for calls (empty = the system default).
        mic: String = String::new() => string,
        /// The speakers for calls (empty = the system default).
        speakers: String = String::new() => string,
        noise_suppression: bool = true => bool,
        /// The microphone's volume in calls, in percent (`0 ..= 100`); a gain on what is
        /// recorded, before noise suppression.
        mic_volume: u8 = 100 => int(0, 100),
        /// Calls and shared games pass through a relay, so friends never learn the address.
        relay_calls: bool = true => bool,
        /// STUN and TURN servers for calls (`stun:host:port`,
        /// `turn:user:password@host:port`, `turns:…`); empty = the SimpleX apps' public
        /// ones (`chat::media::DEFAULT_ICE_SERVERS`, ADR-0035).
        ice_servers: Vec<String> = Vec::new() => custom(custom::ice_servers),
        share_quality: ShareQuality = ShareQuality::P1080 => word,
        share_fps: ShareFps = ShareFps::F60 => word,
        /// Encode a shared game on the GPU (NVENC, VA-API) when there is one; off = always in
        /// software (ADR-0036).
        share_hw_encode: bool = true => bool,
        share_sound: bool = true => bool,
        /// Sharing pauses (a still picture) while the game is not in front.
        share_pause: bool = true => bool,
        notify_calls: bool = true => bool,
        /// The in-game overlay (ADR-0037): started with a game, opened with the overlay
        /// hotkey. Off until its design is ported.
        overlay: bool = false => bool,
        /// New messages appear as toasts over the game while the overlay is closed.
        overlay_toasts: bool = true => bool,
        /// The call's widget stays over the game while the overlay is closed.
        overlay_voice_widget: bool = true => bool,
        overlay_corner: OverlayCorner = OverlayCorner::TopLeft => word,
        /// The key the user bound to `zaun overlay` in their desktop, as the overlay's
        /// hints show it ("Shift+F1"). Only a label: the desktop owns the binding. Empty = no
        /// key ("Open the overlay to reply").
        overlay_key: String = "Shift+F1".to_owned() => string,
    }

    /// `[protection]`.
    Protection "protection" {
        /// Preset for new games (only the three presets).
        default_preset: BasePreset = BasePreset::Offline => custom(custom::base_preset),
        always_isolate_runtime: bool = false => bool,
        /// Network during first-time preparation of a pristine prefix.
        prepare_online: bool = true => bool,
        seccomp_default: bool = true => bool,
        warn_on_x11: bool = true => bool,
        controller_access_default: ControllerDefault = ControllerDefault::Detected => word,
    }

    /// `[runtime]`.
    RuntimePrefs "runtime" {
        /// Launcher-wide compatibility layer.
        default: RuntimeDefault = RuntimeDefault::UmuProton => custom(custom::runtime_default),
        auto_update: bool = true => bool,
        /// `6 ..= 720`.
        auto_update_interval_hours: u32 = 24 => int(6, 720),
        auto_remove_unused: bool = false => bool,
        /// Additional directories scanned for Proton builds (absolute, at most 16).
        extra_search_dirs: Vec<PathBuf> = Vec::new() => custom(custom::search_dirs),
    }

    /// `[launch]`.
    LaunchPrefs "launch" {
        /// `3 ..= 60`.
        stop_grace_secs: u8 = 10 => int(3, 60),
        /// `UMU_LOG=1`.
        verbose_launch_log: bool = false => bool,
        /// Variables for every game (at most 32).
        global_env: Vec<EnvVar> = Vec::new() => custom(custom::global_env),
        /// Small progress window when started from the file manager.
        quick_launch_window: bool = true => bool,
        notify_on_exit: bool = false => bool,
        notify_on_crash: bool = true => bool,
    }

    /// `[statistics]`.
    StatisticsPrefs "statistics" {
        enabled: bool = true => bool,
        keep_history_of_removed: bool = true => bool,
        week_starts_on: WeekStart = WeekStart::System => word,
        /// `0 ..= 300`.
        min_session_secs: u32 = 5 => int(0, 300),
    }

    /// `[logging]`.
    LoggingPrefs "logging" {
        level: LogLevel = LogLevel::Info => word,
        /// `1 ..= 512`.
        session_log_cap_mb: u32 = 32 => int(1, 512),
        /// Session logs kept per game.
        keep_session_logs: u32 = 10 => int(1, 1000),
        /// Maximum age of session logs.
        keep_session_logs_days: u32 = 30 => int(1, 3650),
    }

    /// `[integration]`.
    IntegrationPrefs "integration" {
        /// Whether Zaun handles `.exe` (records the choice only).
        handle_exe: HandleExe = HandleExe::Ask => word,
        /// Clicking a card starts the game.
        single_click_play: bool = false => bool,
        gamepad_navigation: bool = true => bool,
        single_key_shortcuts: bool = true => bool,
    }

    /// `[advanced]`.
    AdvancedPrefs "advanced" {
        developer_mode: bool = false => bool,
        /// Takes effect on the next start.
        renderer: RendererPref = RendererPref::Auto => word,
    }

    /// `[paths]`: overrides of detection; `None` is the empty string in the file.
    PathPrefs "paths" {
        #[serde(serialize_with = "path_or_empty")]
        umu_run: Option<PathBuf> = None => custom(custom::executable),
        #[serde(serialize_with = "path_or_empty")]
        bwrap: Option<PathBuf> = None => custom(custom::executable),
    }

    /// `[onboarding]`.
    OnboardingPrefs "onboarding" {
        completed: bool = false => bool,
    }
}

/// One entry of `appearance.list_column_widths`: `{ column = "last_played", width = 120 }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnWidth {
    /// One of [`ColumnWidth::COLUMNS`].
    pub column: String,
    /// Width in points, [`ColumnWidth::MIN`]`..=`[`ColumnWidth::MAX`].
    pub width: u32,
}

impl ColumnWidth {
    /// The list's optional columns.
    pub const COLUMNS: [&'static str; 6] = [
        "protection",
        "last_played",
        "playtime",
        "tags",
        "layer",
        "added",
    ];
    /// Narrowest width.
    pub const MIN: u32 = 60;
    /// Widest width.
    pub const MAX: u32 = 360;

    /// The preference value of `widths` (for a `PreferencePatch`).
    pub fn to_value(widths: &[ColumnWidth]) -> toml::Value {
        toml::Value::Array(
            widths
                .iter()
                .map(|w| {
                    let mut t = toml::map::Map::new();
                    t.insert("column".into(), toml::Value::String(w.column.clone()));
                    t.insert("width".into(), toml::Value::Integer(i64::from(w.width)));
                    toml::Value::Table(t)
                })
                .collect(),
        )
    }
}

fn path_or_empty<S: serde::Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    match p {
        Some(p) => s.serialize_str(&p.to_string_lossy()),
        None => s.serialize_str(""),
    }
}

/// Keys of `config.toml` this build does not know, kept for rewrite.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnknownKeys(pub(crate) toml::Table);

impl UnknownKeys {
    /// Dotted names of the unknown keys (for the one-time log line).
    pub fn names(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (k, v) in &self.0 {
            match v {
                toml::Value::Table(inner) if super::load::is_known_section(k) => {
                    out.extend(inner.keys().map(|i| format!("{k}.{i}")));
                }
                _ => out.push(k.clone()),
            }
        }
        out
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Global preferences (`config.toml`, CONFIGURATION §2).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Preferences {
    /// File format version, [`CONFIG_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// `[general]`.
    pub general: General,
    /// `[appearance]`.
    pub appearance: Appearance,
    /// `[artwork]`.
    pub artwork: ArtworkPrefs,
    /// `[steam]`.
    pub steam: SteamPrefs,
    /// `[chat]`.
    pub chat: ChatPrefs,
    /// `[protection]`.
    pub protection: Protection,
    /// `[runtime]`.
    pub runtime: RuntimePrefs,
    /// `[launch]`.
    pub launch: LaunchPrefs,
    /// `[statistics]`.
    pub statistics: StatisticsPrefs,
    /// `[logging]`.
    pub logging: LoggingPrefs,
    /// `[integration]`.
    pub integration: IntegrationPrefs,
    /// `[advanced]`.
    pub advanced: AdvancedPrefs,
    /// `[paths]`.
    pub paths: PathPrefs,
    /// `[onboarding]`.
    pub onboarding: OnboardingPrefs,
    /// Keys this build does not know.
    #[serde(skip)]
    pub unknown: UnknownKeys,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            general: General::default(),
            appearance: Appearance::default(),
            artwork: ArtworkPrefs::default(),
            steam: SteamPrefs::default(),
            chat: ChatPrefs::default(),
            protection: Protection::default(),
            runtime: RuntimePrefs::default(),
            launch: LaunchPrefs::default(),
            statistics: StatisticsPrefs::default(),
            logging: LoggingPrefs::default(),
            integration: IntegrationPrefs::default(),
            advanced: AdvancedPrefs::default(),
            paths: PathPrefs::default(),
            onboarding: OnboardingPrefs::default(),
            unknown: UnknownKeys::default(),
        }
    }
}

/// A partial update: dotted keys with new values (`appearance.theme = "dark"`), validated
/// exactly like the file. Unknown keys and invalid values are rejected (LOGIC_SPECS §10).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreferencePatch {
    pub(crate) entries: Vec<(String, toml::Value)>,
}

impl PreferencePatch {
    /// An empty patch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set `key` (dotted, e.g. `launch.stop_grace_secs`) to `value`.
    #[must_use]
    pub fn set(mut self, key: impl Into<String>, value: impl Into<toml::Value>) -> Self {
        self.entries.push((key.into(), value.into()));
        self
    }

    /// The keys touched.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// Whether the patch is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
