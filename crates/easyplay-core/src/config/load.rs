//! Reading, validating and writing `config.toml` (CONFIGURATION §2.1, LOGIC_SPECS §10, §15).
//!
//! The file is parsed into a TOML table and read key by key: a value of the wrong type or
//! outside its enumeration is replaced by the default, a number outside its range is
//! clamped, a bad list entry is dropped, and each of these produces one [`ConfigWarning`].
//! Unknown keys are kept for rewrite (and listed for the log), never warned about. An
//! unparsable file is renamed aside (`config.toml.invalid-<ts>`) and defaults are used; a
//! file from a newer EasyPlay is left untouched and defaults are used read-only. Saving
//! rewrites the whole file atomically with a header comment; comments are not preserved.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use toml::Value;

use super::migrate;
use super::schema::{
    Accent, AdvancedPrefs, Appearance, CloseBehavior, ControllerDefault, General, GridDensity,
    HandleExe, IntegrationPrefs, LaunchPrefs, LibraryView, LogLevel, LoggingPrefs, OnboardingPrefs,
    PathPrefs, PreferencePatch, Preferences, Protection, ReduceMotion, RendererPref,
    RuntimeDefault, RuntimePrefs, StatisticsPrefs, Theme, UnknownKeys, WeekStart,
};
use crate::clock::Timestamp;
use crate::consts::CONFIG_SCHEMA_VERSION;
use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, FsError, FsErrorKind};
use crate::model::{BasePreset, EnvVar};

/// The header written at the top of the file.
pub const HEADER: &str = "# EasyPlay preferences. Edited by the app; comments are not preserved.\n";
/// Largest `config.toml` read.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// `runtime.extra_search_dirs` limit.
pub const MAX_EXTRA_SEARCH_DIRS: usize = 16;
/// `launch.global_env` limit.
pub const MAX_GLOBAL_ENV: usize = 32;
/// Directories never accepted as Proton search directories.
const SEARCH_DIR_DENY: &[&str] = &[
    "/", "/proc", "/sys", "/dev", "/run", "/boot", "/etc", "/tmp",
];

const SECTIONS: &[&str] = &[
    "general",
    "appearance",
    "protection",
    "runtime",
    "launch",
    "statistics",
    "logging",
    "integration",
    "advanced",
    "paths",
    "onboarding",
];

/// Whether `name` is a section this build knows.
pub fn is_known_section(name: &str) -> bool {
    SECTIONS.contains(&name)
}

/// How an environment variable name is treated (UMU_INTEGRATION §4 table D). The table
/// itself belongs to `umu::env`; callers pass it in [`LoadOptions::env_rule`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvRule {
    /// May be set.
    Allowed,
    /// Allowed only with Developer Mode.
    Dangerous,
    /// Never settable.
    Reserved,
}

/// Inputs of validation that live outside `config`.
#[derive(Clone, Copy, Default)]
pub struct LoadOptions<'a> {
    /// Classifier for `launch.global_env` names; `None` checks syntax only.
    pub env_rule: Option<&'a dyn Fn(&str) -> EnvRule>,
}

impl std::fmt::Debug for LoadOptions<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadOptions")
            .field("env_rule", &self.env_rule.is_some())
            .finish()
    }
}

/// What is wrong with a setting.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigProblem {
    /// Wrong type or not one of the allowed values; the default is used.
    InvalidValue {
        /// Why.
        reason: String,
    },
    /// Outside its range; clamped.
    Clamped {
        /// The value used.
        to: String,
    },
    /// A list entry was dropped.
    DroppedEntry {
        /// Position in the list.
        index: usize,
        /// Why.
        reason: String,
    },
    /// The file could not be parsed and was renamed aside.
    Unparsable {
        /// Parser message.
        reason: String,
        /// Where the file went.
        moved_to: Option<PathBuf>,
    },
    /// The file was written by a newer EasyPlay; defaults are used without changing it.
    NewerVersion {
        /// Its `schema_version`.
        found: i64,
    },
}

/// One problem found while loading (shown in a dismissible banner).
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigWarning {
    /// Dotted key, e.g. `appearance.ui_scale`; empty for whole-file problems.
    pub key: String,
    /// The problem.
    pub problem: ConfigProblem,
}

impl ConfigWarning {
    /// The error code of the banner (ERROR_HANDLING §3).
    pub fn code(&self) -> ErrorCode {
        match self.problem {
            ConfigProblem::Unparsable { .. } => ErrorCode::CfgUnreadable,
            ConfigProblem::NewerVersion { .. } => ErrorCode::CfgNewerVersion,
            _ => ErrorCode::CfgInvalidValue,
        }
    }
}

/// Where the loaded preferences came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigSource {
    /// No file: defaults.
    Missing,
    /// Read from the file.
    File,
    /// Read after upgrading from an older version; the original is kept at `backup`.
    Migrated {
        /// Version of the file before.
        from: u32,
        /// `config.toml.bak-v<from>`.
        backup: PathBuf,
    },
    /// Unparsable: defaults.
    Unparsable,
    /// Newer than this build: defaults, read-only.
    Newer,
}

/// Result of [`load`].
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedPreferences {
    /// The preferences in effect.
    pub prefs: Preferences,
    /// Problems for the banner.
    pub warnings: Vec<ConfigWarning>,
    /// Unknown keys (logged once, kept on save).
    pub unknown_keys: Vec<String>,
    /// Origin.
    pub source: ConfigSource,
}

impl LoadedPreferences {
    /// Whether the file may be rewritten (`false` for a newer file, CONFIGURATION §2.1).
    pub fn writable(&self) -> bool {
        self.source != ConfigSource::Newer
    }
}

/// Errors of the configuration module.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Reading or writing the file failed.
    #[error(transparent)]
    Fs(#[from] FsError),
    /// A patched value is not valid; nothing changed.
    #[error("{key}: {reason}")]
    Invalid {
        /// Dotted key.
        key: String,
        /// Why.
        reason: String,
    },
    /// The preferences could not be serialised.
    #[error("cannot serialise preferences: {0}")]
    Serialize(String),
}

impl From<ConfigError> for Error {
    fn from(e: ConfigError) -> Self {
        match e {
            ConfigError::Fs(fs) => Error::from(fs),
            ConfigError::Invalid { ref key, .. } => {
                let key = key.clone();
                Error::new(ErrorCode::CfgInvalidValue)
                    .with("field", key)
                    .with_source(e)
            }
            ConfigError::Serialize(_) => Error::new(ErrorCode::IntTaskFailed).with_source(e),
        }
    }
}

// ---- Reading -----------------------------------------------------------------------------

struct Reader<'a> {
    fs: &'a dyn Fs,
    opts: &'a LoadOptions<'a>,
    warnings: Vec<ConfigWarning>,
    unknown: toml::Table,
}

struct Section {
    name: &'static str,
    table: toml::Table,
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a number",
        Value::Boolean(_) => "true or false",
        Value::Datetime(_) => "a date",
        Value::Array(_) => "a list",
        Value::Table(_) => "a table",
    }
}

impl Reader<'_> {
    fn warn(&mut self, sec: &Section, key: &str, problem: ConfigProblem) {
        self.warnings.push(ConfigWarning {
            key: format!("{}.{key}", sec.name),
            problem,
        });
    }

    fn invalid(&mut self, sec: &Section, key: &str, reason: impl Into<String>) {
        self.warn(
            sec,
            key,
            ConfigProblem::InvalidValue {
                reason: reason.into(),
            },
        );
    }

    fn section(&mut self, root: &mut toml::Table, name: &'static str) -> Section {
        let table = match root.remove(name) {
            None => toml::Table::new(),
            Some(Value::Table(t)) => t,
            Some(other) => {
                self.warnings.push(ConfigWarning {
                    key: name.into(),
                    problem: ConfigProblem::InvalidValue {
                        reason: format!("expected a table, found {}", type_name(&other)),
                    },
                });
                toml::Table::new()
            }
        };
        Section { name, table }
    }

    fn finish(&mut self, sec: Section) {
        if !sec.table.is_empty() {
            self.unknown
                .insert(sec.name.into(), Value::Table(sec.table));
        }
    }

    fn bool(&mut self, sec: &mut Section, key: &str, default: bool) -> bool {
        match sec.table.remove(key) {
            None => default,
            Some(Value::Boolean(b)) => b,
            Some(v) => {
                self.invalid(
                    sec,
                    key,
                    format!("expected true or false, found {}", type_name(&v)),
                );
                default
            }
        }
    }

    fn word<T: FromStr + Copy>(
        &mut self,
        sec: &mut Section,
        key: &str,
        default: T,
        allowed: &[&str],
    ) -> T {
        match sec.table.remove(key) {
            None => default,
            Some(Value::String(s)) => s.trim().parse().unwrap_or_else(|_| {
                self.invalid(
                    sec,
                    key,
                    format!("{s:?} is not one of {}", allowed.join(", ")),
                );
                default
            }),
            Some(v) => {
                self.invalid(
                    sec,
                    key,
                    format!("expected a string, found {}", type_name(&v)),
                );
                default
            }
        }
    }

    fn int(&mut self, sec: &mut Section, key: &str, default: i64, lo: i64, hi: i64) -> i64 {
        match sec.table.remove(key) {
            None => default,
            Some(Value::Integer(i)) if (lo..=hi).contains(&i) => i,
            Some(Value::Integer(i)) => {
                let to = i.clamp(lo, hi);
                self.warn(sec, key, ConfigProblem::Clamped { to: to.to_string() });
                to
            }
            Some(v) => {
                self.invalid(
                    sec,
                    key,
                    format!("expected an integer, found {}", type_name(&v)),
                );
                default
            }
        }
    }

    fn float(&mut self, sec: &mut Section, key: &str, default: f64, lo: f64, hi: f64) -> f64 {
        let v = match sec.table.remove(key) {
            None => return default,
            Some(Value::Float(f)) => f,
            #[allow(clippy::cast_precision_loss)]
            Some(Value::Integer(i)) => i as f64,
            Some(v) => {
                self.invalid(
                    sec,
                    key,
                    format!("expected a number, found {}", type_name(&v)),
                );
                return default;
            }
        };
        if v.is_nan() {
            self.invalid(sec, key, "not a number");
            return default;
        }
        if (lo..=hi).contains(&v) {
            v
        } else {
            let to = v.clamp(lo, hi);
            self.warn(sec, key, ConfigProblem::Clamped { to: to.to_string() });
            to
        }
    }

    fn string(&mut self, sec: &mut Section, key: &str) -> Option<String> {
        match sec.table.remove(key) {
            None => None,
            Some(Value::String(s)) => Some(s),
            Some(v) => {
                self.invalid(
                    sec,
                    key,
                    format!("expected a string, found {}", type_name(&v)),
                );
                None
            }
        }
    }

    fn list(&mut self, sec: &mut Section, key: &str) -> Vec<Value> {
        match sec.table.remove(key) {
            None => Vec::new(),
            Some(Value::Array(a)) => a,
            Some(v) => {
                self.invalid(
                    sec,
                    key,
                    format!("expected a list, found {}", type_name(&v)),
                );
                Vec::new()
            }
        }
    }

    fn dropped(&mut self, sec: &Section, key: &str, index: usize, reason: impl Into<String>) {
        self.warn(
            sec,
            key,
            ConfigProblem::DroppedEntry {
                index,
                reason: reason.into(),
            },
        );
    }

    fn executable_path(&mut self, sec: &mut Section, key: &str) -> Option<PathBuf> {
        let s = self.string(sec, key)?;
        if s.is_empty() {
            return None;
        }
        let p = PathBuf::from(&s);
        if !p.is_absolute() {
            self.invalid(sec, key, "must be an absolute path");
            return None;
        }
        match self.fs.metadata_follow(&p) {
            Ok(m) if m.is_file() && m.mode & 0o111 != 0 => Some(p),
            Ok(_) => {
                self.invalid(sec, key, "not an executable file");
                None
            }
            Err(e) => {
                self.invalid(sec, key, format!("cannot be used: {:?}", e.kind));
                None
            }
        }
    }
}

fn names<T: Copy>(all: &[T], f: impl Fn(T) -> &'static str) -> Vec<&'static str> {
    all.iter().map(|v| f(*v)).collect()
}

fn valid_language(s: &str) -> bool {
    if s == "system" {
        return true;
    }
    let mut parts = s.split('-');
    let first_ok = parts
        .next()
        .is_some_and(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic()));
    first_ok
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn u32_of(v: i64) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

/// Read preferences from a parsed table, validating every key. Never fails: problems become
/// warnings and the affected keys their defaults. Leftover keys are kept as unknown.
pub fn from_table(
    mut root: toml::Table,
    fs: &dyn Fs,
    opts: &LoadOptions<'_>,
) -> (Preferences, Vec<ConfigWarning>) {
    let d = Preferences::default();
    let mut r = Reader {
        fs,
        opts,
        warnings: Vec::new(),
        unknown: toml::Table::new(),
    };
    root.remove("schema_version");

    let mut s = r.section(&mut root, "general");
    let language = match r.string(&mut s, "language") {
        None => d.general.language.clone(),
        Some(l) if valid_language(&l) => l,
        Some(l) => {
            r.invalid(
                &s,
                "language",
                format!("{l:?} is not \"system\" or a language tag"),
            );
            d.general.language.clone()
        }
    };
    let general = General {
        language,
        close_behavior: r.word(
            &mut s,
            "close_behavior",
            d.general.close_behavior,
            &names(CloseBehavior::ALL, CloseBehavior::as_str),
        ),
        confirm_remove: r.bool(&mut s, "confirm_remove", d.general.confirm_remove),
        show_tool_sessions_in_history: r.bool(
            &mut s,
            "show_tool_sessions_in_history",
            d.general.show_tool_sessions_in_history,
        ),
        check_missing_on_focus: r.bool(
            &mut s,
            "check_missing_on_focus",
            d.general.check_missing_on_focus,
        ),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "appearance");
    let appearance = Appearance {
        theme: r.word(
            &mut s,
            "theme",
            d.appearance.theme,
            &names(Theme::ALL, Theme::as_str),
        ),
        accent: r.word(
            &mut s,
            "accent",
            d.appearance.accent,
            &names(Accent::ALL, Accent::as_str),
        ),
        ui_scale: r.float(&mut s, "ui_scale", d.appearance.ui_scale, 0.75, 2.0),
        reduce_motion: r.word(
            &mut s,
            "reduce_motion",
            d.appearance.reduce_motion,
            &names(ReduceMotion::ALL, ReduceMotion::as_str),
        ),
        library_view: r.word(
            &mut s,
            "library_view",
            d.appearance.library_view,
            &names(LibraryView::ALL, LibraryView::as_str),
        ),
        grid_density: r.word(
            &mut s,
            "grid_density",
            d.appearance.grid_density,
            &names(GridDensity::ALL, GridDensity::as_str),
        ),
        show_playtime_on_cards: r.bool(
            &mut s,
            "show_playtime_on_cards",
            d.appearance.show_playtime_on_cards,
        ),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "protection");
    let protection = Protection {
        default_preset: match r.string(&mut s, "default_preset") {
            None => d.protection.default_preset,
            Some(p) => BasePreset::parse(p.trim()).unwrap_or_else(|| {
                r.invalid(
                    &s,
                    "default_preset",
                    format!("{p:?} is not one of offline, online, standard"),
                );
                BasePreset::Offline
            }),
        },
        always_isolate_runtime: r.bool(
            &mut s,
            "always_isolate_runtime",
            d.protection.always_isolate_runtime,
        ),
        prepare_online: r.bool(&mut s, "prepare_online", d.protection.prepare_online),
        seccomp_default: r.bool(&mut s, "seccomp_default", d.protection.seccomp_default),
        warn_on_x11: r.bool(&mut s, "warn_on_x11", d.protection.warn_on_x11),
        controller_access_default: r.word(
            &mut s,
            "controller_access_default",
            d.protection.controller_access_default,
            &names(ControllerDefault::ALL, ControllerDefault::as_str),
        ),
    };
    r.finish(s);

    // `advanced` first: `launch.global_env` depends on `developer_mode` (cross-field rule).
    let mut s = r.section(&mut root, "advanced");
    let advanced = AdvancedPrefs {
        developer_mode: r.bool(&mut s, "developer_mode", d.advanced.developer_mode),
        renderer: r.word(
            &mut s,
            "renderer",
            d.advanced.renderer,
            &names(RendererPref::ALL, RendererPref::as_str),
        ),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "runtime");
    let default = match r.string(&mut s, "default") {
        None => d.runtime.default.clone(),
        Some(v) => v.trim().parse::<RuntimeDefault>().unwrap_or_else(|e| {
            r.invalid(&s, "default", e);
            d.runtime.default.clone()
        }),
    };
    let auto_update = r.bool(&mut s, "auto_update", d.runtime.auto_update);
    let interval = r.int(
        &mut s,
        "auto_update_interval_hours",
        i64::from(d.runtime.auto_update_interval_hours),
        6,
        720,
    );
    let auto_remove_unused = r.bool(&mut s, "auto_remove_unused", d.runtime.auto_remove_unused);
    let mut extra_search_dirs = Vec::new();
    for (i, v) in r.list(&mut s, "extra_search_dirs").into_iter().enumerate() {
        let Value::String(p) = v else {
            r.dropped(&s, "extra_search_dirs", i, "not a string");
            continue;
        };
        let path = PathBuf::from(&p);
        let normal = crate::paths::normalize(&path);
        if !path.is_absolute() {
            r.dropped(&s, "extra_search_dirs", i, "not an absolute path");
        } else if SEARCH_DIR_DENY.iter().any(|d| normal == Path::new(d)) {
            r.dropped(&s, "extra_search_dirs", i, "a system directory");
        } else if extra_search_dirs.len() >= MAX_EXTRA_SEARCH_DIRS {
            r.dropped(
                &s,
                "extra_search_dirs",
                i,
                format!("more than {MAX_EXTRA_SEARCH_DIRS} entries"),
            );
        } else if !extra_search_dirs.contains(&normal) {
            extra_search_dirs.push(normal);
        }
    }
    let runtime = RuntimePrefs {
        default,
        auto_update,
        auto_update_interval_hours: u32_of(interval),
        auto_remove_unused,
        extra_search_dirs,
    };
    r.finish(s);

    let mut s = r.section(&mut root, "launch");
    let stop_grace = r.int(
        &mut s,
        "stop_grace_secs",
        i64::from(d.launch.stop_grace_secs),
        3,
        60,
    );
    let verbose_launch_log = r.bool(&mut s, "verbose_launch_log", d.launch.verbose_launch_log);
    let mut global_env: Vec<EnvVar> = Vec::new();
    for (i, v) in r.list(&mut s, "global_env").into_iter().enumerate() {
        let var: EnvVar = match v.try_into() {
            Ok(var) => var,
            Err(_) => {
                r.dropped(
                    &s,
                    "global_env",
                    i,
                    "expected { name = \"…\", value = \"…\" }",
                );
                continue;
            }
        };
        if let Err(e) = var.validate() {
            r.dropped(&s, "global_env", i, format!("{:?}", e.problem));
            continue;
        }
        let rule = r.opts.env_rule.map_or(EnvRule::Allowed, |f| f(&var.name));
        if rule == EnvRule::Reserved {
            r.dropped(&s, "global_env", i, format!("{} is reserved", var.name));
        } else if rule == EnvRule::Dangerous && !advanced.developer_mode {
            r.dropped(
                &s,
                "global_env",
                i,
                format!("{} requires Developer Mode", var.name),
            );
        } else if global_env.len() >= MAX_GLOBAL_ENV {
            r.dropped(
                &s,
                "global_env",
                i,
                format!("more than {MAX_GLOBAL_ENV} entries"),
            );
        } else if global_env.iter().any(|e| e.name == var.name) {
            r.dropped(&s, "global_env", i, format!("{} is set twice", var.name));
        } else {
            global_env.push(var);
        }
    }
    let launch = LaunchPrefs {
        stop_grace_secs: u8::try_from(stop_grace).unwrap_or(d.launch.stop_grace_secs),
        verbose_launch_log,
        global_env,
        quick_launch_window: r.bool(&mut s, "quick_launch_window", d.launch.quick_launch_window),
        notify_on_exit: r.bool(&mut s, "notify_on_exit", d.launch.notify_on_exit),
        notify_on_crash: r.bool(&mut s, "notify_on_crash", d.launch.notify_on_crash),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "statistics");
    let statistics = StatisticsPrefs {
        enabled: r.bool(&mut s, "enabled", d.statistics.enabled),
        keep_history_of_removed: r.bool(
            &mut s,
            "keep_history_of_removed",
            d.statistics.keep_history_of_removed,
        ),
        week_starts_on: r.word(
            &mut s,
            "week_starts_on",
            d.statistics.week_starts_on,
            &names(WeekStart::ALL, WeekStart::as_str),
        ),
        min_session_secs: u32_of(r.int(
            &mut s,
            "min_session_secs",
            i64::from(d.statistics.min_session_secs),
            0,
            300,
        )),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "logging");
    let logging = LoggingPrefs {
        level: r.word(
            &mut s,
            "level",
            d.logging.level,
            &names(LogLevel::ALL, LogLevel::as_str),
        ),
        session_log_cap_mb: u32_of(r.int(
            &mut s,
            "session_log_cap_mb",
            i64::from(d.logging.session_log_cap_mb),
            1,
            512,
        )),
        keep_session_logs: u32_of(r.int(
            &mut s,
            "keep_session_logs",
            i64::from(d.logging.keep_session_logs),
            1,
            1000,
        )),
        keep_session_logs_days: u32_of(r.int(
            &mut s,
            "keep_session_logs_days",
            i64::from(d.logging.keep_session_logs_days),
            1,
            3650,
        )),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "integration");
    let integration = IntegrationPrefs {
        handle_exe: r.word(
            &mut s,
            "handle_exe",
            d.integration.handle_exe,
            &names(HandleExe::ALL, HandleExe::as_str),
        ),
        single_click_play: r.bool(&mut s, "single_click_play", d.integration.single_click_play),
        gamepad_navigation: r.bool(
            &mut s,
            "gamepad_navigation",
            d.integration.gamepad_navigation,
        ),
        single_key_shortcuts: r.bool(
            &mut s,
            "single_key_shortcuts",
            d.integration.single_key_shortcuts,
        ),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "paths");
    let paths = PathPrefs {
        umu_run: r.executable_path(&mut s, "umu_run"),
        bwrap: r.executable_path(&mut s, "bwrap"),
    };
    r.finish(s);

    let mut s = r.section(&mut root, "onboarding");
    let onboarding = OnboardingPrefs {
        completed: r.bool(&mut s, "completed", d.onboarding.completed),
    };
    r.finish(s);

    // Unknown top-level keys and sections.
    let mut unknown = std::mem::take(&mut r.unknown);
    for (k, v) in root {
        unknown.insert(k, v);
    }

    let prefs = Preferences {
        schema_version: CONFIG_SCHEMA_VERSION,
        general,
        appearance,
        protection,
        runtime,
        launch,
        statistics,
        logging,
        integration,
        advanced,
        paths,
        onboarding,
        unknown: UnknownKeys(unknown),
    };
    (prefs, r.warnings)
}

/// Load `path` (CONFIGURATION §2.1, LOGIC_SPECS §15). Only filesystem failures other than
/// "file missing" are errors.
pub fn load(
    fs: &dyn Fs,
    path: &Path,
    now: Timestamp,
    opts: &LoadOptions<'_>,
) -> Result<LoadedPreferences, ConfigError> {
    let bytes = match fs.read_follow(path, MAX_FILE_BYTES) {
        Ok(b) => b,
        Err(e) if e.kind == FsErrorKind::NotFound => {
            return Ok(LoadedPreferences {
                prefs: Preferences::default(),
                warnings: Vec::new(),
                unknown_keys: Vec::new(),
                source: ConfigSource::Missing,
            });
        }
        Err(e) => return Err(e.into()),
    };
    let parsed = std::str::from_utf8(&bytes)
        .map_err(|e| e.to_string())
        .and_then(|t| t.parse::<toml::Table>().map_err(|e| e.to_string()));
    let table = match parsed {
        Ok(t) => t,
        Err(reason) => return unparsable(fs, path, now, reason),
    };

    let version = match table.get("schema_version") {
        None => i64::from(CONFIG_SCHEMA_VERSION),
        Some(Value::Integer(v)) => *v,
        Some(_) => -1,
    };
    if version > i64::from(CONFIG_SCHEMA_VERSION) {
        return Ok(LoadedPreferences {
            prefs: Preferences::default(),
            warnings: vec![ConfigWarning {
                key: "schema_version".into(),
                problem: ConfigProblem::NewerVersion { found: version },
            }],
            unknown_keys: Vec::new(),
            source: ConfigSource::Newer,
        });
    }
    let mut source = ConfigSource::File;
    let mut table = table;
    if version < i64::from(CONFIG_SCHEMA_VERSION) {
        let from = u32::try_from(version).unwrap_or(0);
        match migrate::upgrade(table, from) {
            Ok(t) => {
                let backup = migrate::backup_path(path, from);
                fs.atomic_write(&backup, &bytes, 0o600)?;
                table = t;
                source = ConfigSource::Migrated { from, backup };
            }
            Err(reason) => return unparsable(fs, path, now, reason),
        }
    }
    let (prefs, warnings) = from_table(table, fs, opts);
    let unknown_keys = prefs.unknown.names();
    if matches!(source, ConfigSource::Migrated { .. }) {
        save(fs, path, &prefs)?;
    }
    Ok(LoadedPreferences {
        prefs,
        warnings,
        unknown_keys,
        source,
    })
}

fn unparsable(
    fs: &dyn Fs,
    path: &Path,
    now: Timestamp,
    reason: String,
) -> Result<LoadedPreferences, ConfigError> {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".invalid-{}", stamp(now)));
    let aside = PathBuf::from(name);
    let moved_to = fs.rename(path, &aside).ok().map(|()| aside);
    Ok(LoadedPreferences {
        prefs: Preferences::default(),
        warnings: vec![ConfigWarning {
            key: String::new(),
            problem: ConfigProblem::Unparsable { reason, moved_to },
        }],
        unknown_keys: Vec::new(),
        source: ConfigSource::Unparsable,
    })
}

fn stamp(now: Timestamp) -> String {
    jiff::Timestamp::from_millisecond(now.as_millis()).map_or_else(
        |_| now.as_millis().to_string(),
        |t| {
            t.to_zoned(jiff::tz::TimeZone::UTC)
                .strftime("%Y%m%dT%H%M%S")
                .to_string()
        },
    )
}

// ---- Writing -----------------------------------------------------------------------------

/// The preferences as a TOML table in documented order, unknown keys merged back.
pub fn to_table(prefs: &Preferences) -> Result<toml::Table, ConfigError> {
    let mut table =
        toml::Table::try_from(prefs).map_err(|e| ConfigError::Serialize(e.to_string()))?;
    for (k, v) in &prefs.unknown.0 {
        match (table.get_mut(k), v) {
            (Some(Value::Table(known)), Value::Table(extra)) => {
                for (ik, iv) in extra {
                    known.entry(ik.clone()).or_insert_with(|| iv.clone());
                }
            }
            (None, _) => {
                table.insert(k.clone(), v.clone());
            }
            _ => {}
        }
    }
    Ok(table)
}

/// The file text: header plus the table.
pub fn to_toml_string(prefs: &Preferences) -> Result<String, ConfigError> {
    let table = to_table(prefs)?;
    let body = toml::to_string(&table).map_err(|e| ConfigError::Serialize(e.to_string()))?;
    Ok(format!("{HEADER}{body}"))
}

/// Write `config.toml` atomically (mode 0600). The caller must not save preferences whose
/// [`LoadedPreferences::writable`] is false.
pub fn save(fs: &dyn Fs, path: &Path, prefs: &Preferences) -> Result<(), ConfigError> {
    let text = to_toml_string(prefs)?;
    fs.atomic_write(path, text.as_bytes(), 0o600)?;
    Ok(())
}

impl Preferences {
    /// Apply a patch: every key must exist and every value be valid, otherwise nothing
    /// changes and the first offending key is reported (`EP-CFG-001`).
    pub fn apply(
        &self,
        patch: &PreferencePatch,
        fs: &dyn Fs,
        opts: &LoadOptions<'_>,
    ) -> Result<Preferences, ConfigError> {
        let known = to_table(&Preferences::default())?;
        let mut table = to_table(self)?;
        for (key, value) in &patch.entries {
            let invalid = |reason: &str| ConfigError::Invalid {
                key: key.clone(),
                reason: reason.to_owned(),
            };
            let (sec, name) = key.split_once('.').ok_or_else(|| invalid("unknown key"))?;
            let is_known = known
                .get(sec)
                .and_then(Value::as_table)
                .is_some_and(|t| t.contains_key(name));
            if !is_known {
                return Err(invalid("unknown key"));
            }
            if let Some(Value::Table(t)) = table.get_mut(sec) {
                t.insert(name.to_owned(), value.clone());
            }
        }
        let (next, warnings) = from_table(table, fs, opts);
        let touched: Vec<&str> = patch.keys().collect();
        if let Some(w) = warnings.iter().find(|w| touched.contains(&w.key.as_str())) {
            let reason = match &w.problem {
                ConfigProblem::InvalidValue { reason } => reason.clone(),
                ConfigProblem::Clamped { to } => format!("out of range (nearest allowed: {to})"),
                ConfigProblem::DroppedEntry { reason, .. } => reason.clone(),
                other => format!("{other:?}"),
            };
            return Err(ConfigError::Invalid {
                key: w.key.clone(),
                reason,
            });
        }
        Ok(next)
    }
}
