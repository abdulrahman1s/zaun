//! Reading, validating and writing `config.toml` (CONFIGURATION §2.1, LOGIC_SPECS §10, §15).
//!
//! The file is parsed into a TOML table and read key by key: a value of the wrong type or
//! outside its enumeration is replaced by the default, a number outside its range is
//! clamped, a bad list entry is dropped, and each of these produces one [`ConfigWarning`].
//! Unknown keys are kept for rewrite (and listed for the log), never warned about. An
//! unparsable file is renamed aside (`config.toml.invalid-<ts>`) and defaults are used; a
//! file from a newer Zaun is left untouched and defaults are used read-only. Saving
//! rewrites the whole file atomically with a header comment; comments are not preserved.

use std::path::{Path, PathBuf};

use toml::Value;

use super::migrate;
use super::reader::Reader;
use super::schema::{
    AdvancedPrefs, Appearance, ArtworkPrefs, ChatPrefs, General, IntegrationPrefs, LaunchPrefs,
    LoggingPrefs, OnboardingPrefs, PathPrefs, PreferencePatch, Preferences, Protection,
    RuntimePrefs, StatisticsPrefs, SteamPrefs, UnknownKeys, UpdatePrefs,
};
use crate::clock::Timestamp;
use crate::consts::CONFIG_SCHEMA_VERSION;
use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, FsError, FsErrorKind};

/// The header written at the top of the file.
pub const HEADER: &str = "# Zaun preferences. Edited by the app; comments are not preserved.\n";
/// Largest `config.toml` read.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// `runtime.extra_search_dirs` limit.
pub const MAX_EXTRA_SEARCH_DIRS: usize = 16;
/// `launch.global_env` limit.
pub const MAX_GLOBAL_ENV: usize = 32;
/// Directories never accepted as Proton search directories.
pub(super) const SEARCH_DIR_DENY: &[&str] = &[
    "/", "/proc", "/sys", "/dev", "/run", "/boot", "/etc", "/tmp",
];

const SECTIONS: &[&str] = &[
    "general",
    "appearance",
    "artwork",
    "steam",
    "chat",
    "protection",
    "runtime",
    "launch",
    "statistics",
    "logging",
    "integration",
    "advanced",
    "paths",
    "updates",
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
    /// The file was written by a newer Zaun; defaults are used without changing it.
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

/// Read preferences from a parsed table, validating every key. Never fails: problems become
/// warnings and the affected keys their defaults. Leftover keys are kept as unknown.
pub fn from_table(
    mut root: toml::Table,
    fs: &dyn Fs,
    opts: &LoadOptions<'_>,
) -> (Preferences, Vec<ConfigWarning>) {
    let mut r = Reader::new(fs, opts);
    root.remove("schema_version");

    let general = General::read(&mut r, &mut root);
    let appearance = Appearance::read(&mut r, &mut root);
    let artwork = ArtworkPrefs::read(&mut r, &mut root);
    let steam = SteamPrefs::read(&mut r, &mut root);
    let chat = ChatPrefs::read(&mut r, &mut root);
    let protection = Protection::read(&mut r, &mut root);
    // `advanced` first: `launch.global_env` depends on `developer_mode` (cross-field rule).
    let advanced = AdvancedPrefs::read(&mut r, &mut root);
    r.developer_mode = advanced.developer_mode;
    let runtime = RuntimePrefs::read(&mut r, &mut root);
    let launch = LaunchPrefs::read(&mut r, &mut root);
    let statistics = StatisticsPrefs::read(&mut r, &mut root);
    let logging = LoggingPrefs::read(&mut r, &mut root);
    let integration = IntegrationPrefs::read(&mut r, &mut root);
    let paths = PathPrefs::read(&mut r, &mut root);
    let updates = UpdatePrefs::read(&mut r, &mut root);
    let onboarding = OnboardingPrefs::read(&mut r, &mut root);

    // Unknown top-level keys and sections.
    let mut unknown = std::mem::take(&mut r.unknown);
    unknown.extend(root);

    let prefs = Preferences {
        schema_version: CONFIG_SCHEMA_VERSION,
        general,
        appearance,
        artwork,
        steam,
        chat,
        protection,
        runtime,
        launch,
        statistics,
        logging,
        integration,
        advanced,
        paths,
        updates,
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
    // The first save on a fresh account creates `~/.config/zaun`.
    if let Some(dir) = path.parent() {
        fs.create_dir_all(dir, 0o700)?;
    }
    fs.atomic_write(path, text.as_bytes(), 0o600)?;
    Ok(())
}

impl Preferences {
    /// Apply a patch: every key must exist and every value be valid, otherwise nothing
    /// changes and the first offending key is reported (`ZN-CFG-001`).
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
