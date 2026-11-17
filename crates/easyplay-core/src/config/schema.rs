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

use crate::consts::CONFIG_SCHEMA_VERSION;
use crate::ids::RuntimeName;
use crate::model::{BasePreset, EnvVar};

macro_rules! words {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident = $s:literal),+ $(,)? } default $d:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $( $(#[$vm])* #[serde(rename = $s)] $v, )+
        }
        impl $name {
            /// Every value, in documented order.
            pub const ALL: &'static [$name] = &[$($name::$v),+];
            /// The word used in `config.toml`.
            pub const fn as_str(self) -> &'static str {
                match self { $($name::$v => $s),+ }
            }
        }
        impl Default for $name {
            fn default() -> Self { $name::$d }
        }
        impl FromStr for $name {
            type Err = ();
            fn from_str(s: &str) -> Result<Self, ()> {
                Self::ALL.iter().copied().find(|v| v.as_str().eq_ignore_ascii_case(s)).ok_or(())
            }
        }
    };
}

words!(
    /// `general.close_behavior`.
    CloseBehavior { Quit = "quit", AskIfRunning = "ask-if-running" } default Quit
);
words!(
    /// `appearance.theme`.
    Theme { System = "system", Dark = "dark", Light = "light", HighContrast = "high-contrast" } default System
);
words!(
    /// `appearance.accent`.
    Accent { Tide = "tide", Ember = "ember", Iris = "iris", Moss = "moss" } default Tide
);
words!(
    /// `appearance.reduce_motion`.
    ReduceMotion { System = "system", On = "on", Off = "off" } default System
);
words!(
    /// `appearance.library_view`.
    LibraryView { Grid = "grid", List = "list" } default Grid
);
words!(
    /// `appearance.grid_density`.
    GridDensity { Compact = "compact", Comfortable = "comfortable", Large = "large" } default Comfortable
);
words!(
    /// `artwork.online`: where artwork may be fetched from (ADR-0018).
    ArtworkOnline { Off = "off", Steam = "steam" } default Off
);
words!(
    /// `protection.controller_access_default`.
    ControllerDefault { Detected = "detected", AllWhenSafe = "all-when-safe" } default Detected
);
words!(
    /// `statistics.week_starts_on`.
    WeekStart { System = "system", Monday = "monday", Sunday = "sunday" } default System
);
words!(
    /// `logging.level`.
    LogLevel { Error = "error", Warn = "warn", Info = "info", Debug = "debug", Trace = "trace" } default Info
);
words!(
    /// `integration.handle_exe`.
    HandleExe { Ask = "ask", Yes = "yes", No = "no" } default Ask
);
words!(
    /// `advanced.renderer`.
    RendererPref { Auto = "auto", Wgpu = "wgpu", Glow = "glow" } default Auto
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

/// `[general]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct General {
    /// `"system"` or a BCP-47 tag.
    pub language: String,
    /// Window close behaviour.
    pub close_behavior: CloseBehavior,
    /// Confirm before removing a game.
    pub confirm_remove: bool,
    /// Show tool sessions in history.
    pub show_tool_sessions_in_history: bool,
    /// Re-check missing games when the window gains focus.
    pub check_missing_on_focus: bool,
}

/// `[appearance]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Appearance {
    /// Theme.
    pub theme: Theme,
    /// Accent colour.
    pub accent: Accent,
    /// `0.75 ..= 2.0`, multiplies the system scale.
    pub ui_scale: f64,
    /// Reduced motion.
    pub reduce_motion: ReduceMotion,
    /// Initial library view.
    pub library_view: LibraryView,
    /// Grid density.
    pub grid_density: GridDensity,
    /// Playtime on cards.
    pub show_playtime_on_cards: bool,
    /// Widths of the library list's optional columns (absent = the default width).
    pub list_column_widths: Vec<ColumnWidth>,
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

/// `[artwork]` (ADR-0018).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ArtworkPrefs {
    /// `"off"` (never contact a server) or `"steam"` (fetch from the Steam CDN).
    pub online: ArtworkOnline,
    /// With `online = "steam"` and no known app ID, the game's display name may be sent to
    /// Steam's store search.
    pub name_search: bool,
}

/// `[steam]` (ADR-0031).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SteamPrefs {
    /// "Show my Steam games": list the games installed with Steam on this computer. Off by
    /// default; while off, nothing of Steam's is read.
    pub show_games: bool,
}

/// `[protection]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Protection {
    /// Preset for new games (only the three presets).
    pub default_preset: BasePreset,
    /// Force Isolated runtime mode.
    pub always_isolate_runtime: bool,
    /// Network during first-time preparation of a pristine prefix.
    pub prepare_online: bool,
    /// seccomp filter by default.
    pub seccomp_default: bool,
    /// Warn when a game uses X11.
    pub warn_on_x11: bool,
    /// Controller access for new games.
    pub controller_access_default: ControllerDefault,
}

/// `[runtime]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuntimePrefs {
    /// Launcher-wide compatibility layer.
    pub default: RuntimeDefault,
    /// Update automatically.
    pub auto_update: bool,
    /// `6 ..= 720`.
    pub auto_update_interval_hours: u32,
    /// Remove unused builds.
    pub auto_remove_unused: bool,
    /// Additional directories scanned for Proton builds (absolute, at most 16).
    pub extra_search_dirs: Vec<PathBuf>,
}

/// `[launch]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaunchPrefs {
    /// `3 ..= 60`.
    pub stop_grace_secs: u8,
    /// `UMU_LOG=1`.
    pub verbose_launch_log: bool,
    /// Variables for every game (at most 32).
    pub global_env: Vec<EnvVar>,
    /// Small progress window when started from the file manager.
    pub quick_launch_window: bool,
    /// Notify when a game ends in the background.
    pub notify_on_exit: bool,
    /// Notify on crash.
    pub notify_on_crash: bool,
}

/// `[statistics]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatisticsPrefs {
    /// Statistics shown and aggregated.
    pub enabled: bool,
    /// Keep history of removed games.
    pub keep_history_of_removed: bool,
    /// First day of the week.
    pub week_starts_on: WeekStart,
    /// `0 ..= 300`.
    pub min_session_secs: u32,
}

/// `[logging]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LoggingPrefs {
    /// Level.
    pub level: LogLevel,
    /// `1 ..= 512`.
    pub session_log_cap_mb: u32,
    /// Session logs kept per game.
    pub keep_session_logs: u32,
    /// Maximum age of session logs.
    pub keep_session_logs_days: u32,
}

/// `[integration]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IntegrationPrefs {
    /// Whether EasyPlay handles `.exe` (records the choice only).
    pub handle_exe: HandleExe,
    /// Clicking a card starts the game.
    pub single_click_play: bool,
    /// Gamepad navigation.
    pub gamepad_navigation: bool,
    /// Single-key shortcuts.
    pub single_key_shortcuts: bool,
}

/// `[advanced]`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AdvancedPrefs {
    /// Developer Mode.
    pub developer_mode: bool,
    /// Renderer (next start).
    pub renderer: RendererPref,
}

/// `[paths]`: overrides of detection; `None` is the empty string in the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PathPrefs {
    /// `umu-run`.
    #[serde(serialize_with = "path_or_empty")]
    pub umu_run: Option<PathBuf>,
    /// `bwrap`.
    #[serde(serialize_with = "path_or_empty")]
    pub bwrap: Option<PathBuf>,
}

fn path_or_empty<S: serde::Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    match p {
        Some(p) => s.serialize_str(&p.to_string_lossy()),
        None => s.serialize_str(""),
    }
}

/// `[onboarding]`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OnboardingPrefs {
    /// Onboarding finished.
    pub completed: bool,
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
            general: General {
                language: "system".into(),
                close_behavior: CloseBehavior::Quit,
                confirm_remove: true,
                show_tool_sessions_in_history: false,
                check_missing_on_focus: true,
            },
            appearance: Appearance {
                theme: Theme::System,
                accent: Accent::Tide,
                ui_scale: 1.0,
                reduce_motion: ReduceMotion::System,
                library_view: LibraryView::Grid,
                grid_density: GridDensity::Comfortable,
                show_playtime_on_cards: true,
                list_column_widths: Vec::new(),
            },
            artwork: ArtworkPrefs {
                online: ArtworkOnline::Off,
                name_search: true,
            },
            steam: SteamPrefs { show_games: false },
            protection: Protection {
                default_preset: BasePreset::Offline,
                always_isolate_runtime: false,
                prepare_online: true,
                seccomp_default: true,
                warn_on_x11: true,
                controller_access_default: ControllerDefault::Detected,
            },
            runtime: RuntimePrefs {
                default: RuntimeDefault::UmuProton,
                auto_update: true,
                auto_update_interval_hours: 24,
                auto_remove_unused: false,
                extra_search_dirs: Vec::new(),
            },
            launch: LaunchPrefs {
                stop_grace_secs: 10,
                verbose_launch_log: false,
                global_env: Vec::new(),
                quick_launch_window: true,
                notify_on_exit: false,
                notify_on_crash: true,
            },
            statistics: StatisticsPrefs {
                enabled: true,
                keep_history_of_removed: true,
                week_starts_on: WeekStart::System,
                min_session_secs: 5,
            },
            logging: LoggingPrefs {
                level: LogLevel::Info,
                session_log_cap_mb: 32,
                keep_session_logs: 10,
                keep_session_logs_days: 30,
            },
            integration: IntegrationPrefs {
                handle_exe: HandleExe::Ask,
                single_click_play: false,
                gamepad_navigation: true,
                single_key_shortcuts: true,
            },
            advanced: AdvancedPrefs {
                developer_mode: false,
                renderer: RendererPref::Auto,
            },
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
