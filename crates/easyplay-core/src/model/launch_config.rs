//! Per-game launch configuration (DATA_MODEL §4, CONFIGURATION §3) and argument quoting.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Problem, ValidationError};
use crate::consts::{
    LAUNCH_SCHEMA_VERSION, MAX_DLL_OVERRIDES_BYTES, MAX_DXVK_HUD_BYTES, MAX_ENV_NAME_BYTES,
    MAX_ENV_VALUE_BYTES, MAX_LAUNCH_ARG_BYTES, MAX_LAUNCH_ARGS, MAX_LAUNCH_ENV,
    MAX_WINE_DEBUG_BYTES, STOP_GRACE_DEFAULT, STOP_GRACE_MAX_SECS, STOP_GRACE_MIN_SECS,
};

/// How a game is started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchConfig {
    /// Document version, [`LAUNCH_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Arguments; each ≤ 4096 bytes, no NUL; at most 128. Authoritative over the text form.
    pub args: Vec<String>,
    /// Working directory.
    pub working_dir: WorkingDir,
    /// Extra environment variables; at most 64. Name rules beyond syntax are `umu::env`'s.
    pub env: Vec<EnvVar>,
    /// Compatibility options.
    pub compat: CompatOptions,
    /// Grace period of a graceful stop, `3..=60`, default 10.
    pub stop_grace_secs: u8,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            schema_version: LAUNCH_SCHEMA_VERSION,
            args: Vec::new(),
            working_dir: WorkingDir::ExeDir,
            env: Vec::new(),
            compat: CompatOptions::default(),
            // The default is 10 s and fits in u8.
            stop_grace_secs: STOP_GRACE_DEFAULT.as_secs() as u8,
        }
    }
}

impl LaunchConfig {
    /// Check every limit of DATA_MODEL §4.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version == 0 || self.schema_version > LAUNCH_SCHEMA_VERSION {
            return Err(ValidationError::new(
                "launch.schema_version",
                Problem::UnsupportedVersion {
                    found: self.schema_version,
                    max: LAUNCH_SCHEMA_VERSION,
                },
            ));
        }
        validate_args("launch.args", &self.args)?;
        if let WorkingDir::Custom(dir) = &self.working_dir
            && !dir.is_absolute()
        {
            return Err(ValidationError::new(
                "launch.working_dir",
                Problem::NotAbsolute,
            ));
        }
        if self.env.len() > MAX_LAUNCH_ENV {
            return Err(ValidationError::new(
                "launch.env",
                Problem::TooMany {
                    max: MAX_LAUNCH_ENV,
                },
            ));
        }
        for var in &self.env {
            var.validate()?;
        }
        self.compat.validate()?;
        if !(STOP_GRACE_MIN_SECS..=STOP_GRACE_MAX_SECS).contains(&self.stop_grace_secs) {
            return Err(ValidationError::new(
                "launch.stop_grace_secs",
                Problem::OutOfRange,
            ));
        }
        Ok(())
    }
}

/// Validate an argument list (stored arguments, `args_override`, and extra CLI arguments).
pub fn validate_args(field: &'static str, args: &[String]) -> Result<(), ValidationError> {
    if args.len() > MAX_LAUNCH_ARGS {
        return Err(ValidationError::new(
            field,
            Problem::TooMany {
                max: MAX_LAUNCH_ARGS,
            },
        ));
    }
    for arg in args {
        if arg.len() > MAX_LAUNCH_ARG_BYTES {
            return Err(ValidationError::new(
                field,
                Problem::TooLong {
                    max: MAX_LAUNCH_ARG_BYTES,
                },
            ));
        }
        if arg.contains('\0') {
            return Err(ValidationError::new(field, Problem::ContainsNul));
        }
    }
    Ok(())
}

/// The working directory of the started program.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingDir {
    /// The executable's directory.
    ExeDir,
    /// The game root.
    GameRoot,
    /// An explicit absolute directory.
    Custom(PathBuf),
}

/// One environment variable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
}

impl EnvVar {
    /// Syntax and size checks. Whether a name is allowed, reserved or dangerous is decided by
    /// `umu::env` (UMU_INTEGRATION §4 table D).
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_env_name("launch.env.name", &self.name)?;
        if self.value.len() > MAX_ENV_VALUE_BYTES {
            return Err(ValidationError::new(
                "launch.env.value",
                Problem::TooLong {
                    max: MAX_ENV_VALUE_BYTES,
                },
            ));
        }
        if self.value.contains('\0') {
            return Err(ValidationError::new(
                "launch.env.value",
                Problem::ContainsNul,
            ));
        }
        Ok(())
    }
}

/// `^[A-Za-z_][A-Za-z0-9_]*$`, at most 128 bytes.
pub(crate) fn validate_env_name(field: &'static str, name: &str) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::new(field, Problem::Empty));
    }
    if name.len() > MAX_ENV_NAME_BYTES {
        return Err(ValidationError::new(
            field,
            Problem::TooLong {
                max: MAX_ENV_NAME_BYTES,
            },
        ));
    }
    let mut bytes = name.bytes();
    let first_ok = bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    if !first_ok || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(ValidationError::new(field, Problem::InvalidCharacters));
    }
    Ok(())
}

/// Compatibility options translated into umu/Proton variables by `umu::env`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatOptions {
    /// Direct3D translation target.
    pub renderer: Renderer,
    /// Window system Proton draws through.
    pub window_system: WindowSystem,
    /// Synchronisation primitives.
    pub sync: SyncMode,
    /// `PROTONFIXES_DISABLE`.
    pub disable_fixes: bool,
    /// `WINEDLLOVERRIDES`, ≤ 1024 bytes.
    pub dll_overrides: String,
    /// MangoHud overlay.
    pub mangohud: bool,
    /// `DXVK_HUD`, ≤ 128 bytes, `[a-z0-9,=._-]`.
    pub dxvk_hud: Option<String>,
    /// Locale, e.g. `ja_JP.UTF-8`.
    pub locale: Option<String>,
    /// `PROTON_LOG`.
    pub proton_log: bool,
    /// `WINEDEBUG` (Developer Mode).
    pub wine_debug: Option<String>,
}

impl Default for CompatOptions {
    fn default() -> Self {
        Self {
            renderer: Renderer::Vulkan,
            window_system: WindowSystem::Auto,
            sync: SyncMode::Default,
            disable_fixes: false,
            dll_overrides: String::new(),
            mangohud: false,
            dxvk_hud: None,
            locale: None,
            proton_log: false,
            wine_debug: None,
        }
    }
}

impl CompatOptions {
    /// Check the limits of DATA_MODEL §4.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.dll_overrides.len() > MAX_DLL_OVERRIDES_BYTES {
            return Err(ValidationError::new(
                "compat.dll_overrides",
                Problem::TooLong {
                    max: MAX_DLL_OVERRIDES_BYTES,
                },
            ));
        }
        if self.dll_overrides.chars().any(char::is_control) {
            return Err(ValidationError::new(
                "compat.dll_overrides",
                Problem::InvalidCharacters,
            ));
        }
        if let Some(hud) = &self.dxvk_hud {
            if hud.len() > MAX_DXVK_HUD_BYTES {
                return Err(ValidationError::new(
                    "compat.dxvk_hud",
                    Problem::TooLong {
                        max: MAX_DXVK_HUD_BYTES,
                    },
                ));
            }
            let ok = hud
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b",=._-".contains(&b));
            if !ok {
                return Err(ValidationError::new(
                    "compat.dxvk_hud",
                    Problem::InvalidCharacters,
                ));
            }
        }
        if let Some(locale) = &self.locale
            && !is_valid_locale(locale)
        {
            return Err(ValidationError::new(
                "compat.locale",
                Problem::InvalidFormat,
            ));
        }
        if let Some(debug) = &self.wine_debug {
            if debug.len() > MAX_WINE_DEBUG_BYTES {
                return Err(ValidationError::new(
                    "compat.wine_debug",
                    Problem::TooLong {
                        max: MAX_WINE_DEBUG_BYTES,
                    },
                ));
            }
            let ok = debug
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-,_".contains(&b));
            if !ok {
                return Err(ValidationError::new(
                    "compat.wine_debug",
                    Problem::InvalidCharacters,
                ));
            }
        }
        Ok(())
    }
}

/// `^[A-Za-z_]+(\.[A-Za-z0-9-]+)?(@[A-Za-z]+)?$`
pub fn is_valid_locale(s: &str) -> bool {
    let (rest, modifier) = match s.split_once('@') {
        Some((r, m)) => (r, Some(m)),
        None => (s, None),
    };
    if let Some(m) = modifier
        && (m.is_empty() || !m.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return false;
    }
    let (lang, codeset) = match rest.split_once('.') {
        Some((l, c)) => (l, Some(c)),
        None => (rest, None),
    };
    if lang.is_empty() || !lang.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_') {
        return false;
    }
    match codeset {
        Some(c) => !c.is_empty() && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        None => true,
    }
}

/// Direct3D translation target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Renderer {
    /// DXVK/VKD3D (default).
    Vulkan,
    /// WineD3D on OpenGL.
    #[serde(rename = "opengl")]
    OpenGl,
}

/// Window system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSystem {
    /// Proton's default.
    Auto,
    /// Force X11 (Xwayland).
    X11,
    /// Proton's Wayland driver.
    Wayland,
}

/// Synchronisation primitives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Proton's default.
    Default,
    /// `PROTON_NO_FSYNC=1`.
    NoFsync,
    /// `PROTON_NO_ESYNC=1` and `PROTON_NO_FSYNC=1`.
    NoEsyncNoFsync,
}

/// A trusted helper run in a game's prefix (UMU_INTEGRATION §3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// `winecfg`.
    Winecfg,
    /// `regedit`.
    Regedit,
    /// `control`.
    ControlPanel,
    /// `explorer`.
    Explorer,
    /// `winetricks <verbs…>`; verbs `^[A-Za-z0-9_-]+(=[A-Za-z0-9]*)?$`.
    Winetricks {
        /// Verbs.
        verbs: Vec<String>,
    },
    /// Another program, e.g. a patcher.
    RunExe {
        /// Absolute path.
        path: PathBuf,
        /// Arguments.
        args: Vec<String>,
    },
}

impl Tool {
    /// Validate verbs and arguments.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Tool::Winetricks { verbs } => {
                if verbs.is_empty() {
                    return Err(ValidationError::new("tool.verbs", Problem::Empty));
                }
                for verb in verbs {
                    if !is_valid_winetricks_verb(verb) {
                        return Err(ValidationError::new(
                            "tool.verbs",
                            Problem::InvalidCharacters,
                        ));
                    }
                }
                Ok(())
            }
            Tool::RunExe { path, args } => {
                if !path.is_absolute() {
                    return Err(ValidationError::new("tool.path", Problem::NotAbsolute));
                }
                validate_args("tool.args", args)
            }
            Tool::Winecfg | Tool::Regedit | Tool::ControlPanel | Tool::Explorer => Ok(()),
        }
    }
}

/// `^[A-Za-z0-9_-]+(=[A-Za-z0-9]*)?$`
pub fn is_valid_winetricks_verb(verb: &str) -> bool {
    let (name, value) = match verb.split_once('=') {
        Some((n, v)) => (n, Some(v)),
        None => (verb, None),
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && value.is_none_or(|v| v.bytes().all(|b| b.is_ascii_alphanumeric()))
}

// ---- Argument text form -----------------------------------------------------------------

/// Why an argument string could not be split.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SplitError {
    /// A quote was opened but not closed.
    #[error("unterminated quote")]
    UnterminatedQuote,
    /// The text ends with a lone backslash.
    #[error("trailing backslash")]
    TrailingBackslash,
    /// A NUL character.
    #[error("NUL character")]
    Nul,
}

/// Split a single text field into arguments with POSIX-shell-like quoting: whitespace
/// separates, `"…"` and `'…'` group, `\` escapes the next character outside single quotes
/// (inside double quotes only `\"`, `\\`, `` \` `` and `\$` are escapes). No expansion of any
/// kind happens.
pub fn split_args(text: &str) -> Result<Vec<String>, SplitError> {
    #[derive(PartialEq)]
    enum Q {
        None,
        Single,
        Double,
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut q = Q::None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\0' {
            return Err(SplitError::Nul);
        }
        match q {
            Q::Single => {
                if c == '\'' {
                    q = Q::None;
                } else {
                    cur.push(c);
                }
            }
            Q::Double => match c {
                '"' => q = Q::None,
                '\\' => match chars.next() {
                    Some(n @ ('"' | '\\' | '`' | '$')) => cur.push(n),
                    Some('\0') => return Err(SplitError::Nul),
                    Some(n) => {
                        cur.push('\\');
                        cur.push(n);
                    }
                    None => return Err(SplitError::UnterminatedQuote),
                },
                _ => cur.push(c),
            },
            Q::None => match c {
                '\'' => {
                    q = Q::Single;
                    in_word = true;
                }
                '"' => {
                    q = Q::Double;
                    in_word = true;
                }
                '\\' => match chars.next() {
                    Some('\0') => return Err(SplitError::Nul),
                    Some(n) => {
                        cur.push(n);
                        in_word = true;
                    }
                    None => return Err(SplitError::TrailingBackslash),
                },
                c if c.is_whitespace() => {
                    if in_word {
                        out.push(std::mem::take(&mut cur));
                        in_word = false;
                    }
                }
                c => {
                    cur.push(c);
                    in_word = true;
                }
            },
        }
    }
    if q != Q::None {
        return Err(SplitError::UnterminatedQuote);
    }
    if in_word {
        out.push(cur);
    }
    Ok(out)
}

/// Join arguments into the text form; `split_args(&join_args(a)) == a` for every list of
/// NUL-free arguments.
pub fn join_args(args: &[String]) -> String {
    let mut out = String::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let plain = !arg.is_empty()
            && arg.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || "-_./=:,+@%^".contains(c)
                    || (!c.is_ascii() && !c.is_whitespace())
            });
        if plain {
            out.push_str(arg);
        } else {
            out.push('\'');
            for c in arg.chars() {
                if c == '\'' {
                    out.push_str("'\\''");
                } else {
                    out.push(c);
                }
            }
            out.push('\'');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn split_table() {
        let cases: &[(&str, &[&str])] = &[
            ("", &[]),
            ("   ", &[]),
            ("-windowed", &["-windowed"]),
            ("a b  c", &["a", "b", "c"]),
            ("\"a b\" c", &["a b", "c"]),
            ("'a b' c", &["a b", "c"]),
            ("a\\ b", &["a b"]),
            ("x\"y z\"w", &["xy zw"]),
            ("''", &[""]),
            ("\"\"", &[""]),
            ("'it'\\''s'", &["it's"]),
            ("\"say \\\"hi\\\"\"", &["say \"hi\""]),
            ("\"a\\nb\"", &["a\\nb"]),
            ("'$HOME' \"$HOME\"", &["$HOME", "$HOME"]),
            ("--res=1920x1080 -dx12", &["--res=1920x1080", "-dx12"]),
            ("tab\tsep\nnl", &["tab", "sep", "nl"]),
        ];
        for (input, expected) in cases {
            assert_eq!(split_args(input).unwrap(), s(expected), "{input:?}");
        }
    }

    #[test]
    fn split_errors() {
        assert_eq!(split_args("'open"), Err(SplitError::UnterminatedQuote));
        assert_eq!(split_args("\"open"), Err(SplitError::UnterminatedQuote));
        assert_eq!(split_args("end\\"), Err(SplitError::TrailingBackslash));
        assert_eq!(split_args("a\0b"), Err(SplitError::Nul));
    }

    #[test]
    fn join_round_trips() {
        let cases: &[&[&str]] = &[
            &[],
            &["-windowed"],
            &["a b", "c"],
            &[""],
            &["it's", "\"q\"", "back\\slash", "$HOME", "tab\there"],
            &["ünïcode", "日本語 テキスト"],
        ];
        for case in cases {
            let args = s(case);
            let text = join_args(&args);
            assert_eq!(split_args(&text).unwrap(), args, "{text:?}");
        }
        assert_eq!(join_args(&s(&["-a", "b c"])), "-a 'b c'");
    }

    #[test]
    fn launch_config_defaults_and_limits() {
        let mut c = LaunchConfig::default();
        assert_eq!(c.stop_grace_secs, 10);
        c.validate().unwrap();
        c.stop_grace_secs = 2;
        assert_eq!(c.validate().unwrap_err().field, "launch.stop_grace_secs");
        c.stop_grace_secs = 61;
        assert!(c.validate().is_err());
        c.stop_grace_secs = 60;
        c.args = vec!["x".into(); 129];
        assert!(matches!(
            c.validate().unwrap_err().problem,
            Problem::TooMany { max: 128 }
        ));
        c.args = vec!["x".repeat(4097)];
        assert!(c.validate().is_err());
        c.args = vec!["a\0".into()];
        assert_eq!(c.validate().unwrap_err().problem, Problem::ContainsNul);
        c.args.clear();
        c.working_dir = WorkingDir::Custom("relative".into());
        assert_eq!(c.validate().unwrap_err().problem, Problem::NotAbsolute);
    }

    #[test]
    fn env_var_rules() {
        let ok = ["DXVK_HUD", "_X", "a1"];
        let bad = ["", "1A", "A-B", "A B", "Ä"];
        for n in ok {
            assert!(
                EnvVar {
                    name: n.into(),
                    value: "v".into()
                }
                .validate()
                .is_ok(),
                "{n}"
            );
        }
        for n in bad {
            assert!(
                EnvVar {
                    name: n.into(),
                    value: "v".into()
                }
                .validate()
                .is_err(),
                "{n}"
            );
        }
        let c = LaunchConfig {
            env: (0..65)
                .map(|i| EnvVar {
                    name: format!("V{i}"),
                    value: String::new(),
                })
                .collect(),
            ..LaunchConfig::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn compat_rules() {
        let mut o = CompatOptions::default();
        o.validate().unwrap();
        o.dxvk_hud = Some("fps,devinfo".into());
        o.validate().unwrap();
        o.dxvk_hud = Some("FPS".into());
        assert!(o.validate().is_err());
        o.dxvk_hud = None;
        o.dll_overrides = "x".repeat(1025);
        assert!(o.validate().is_err());
        o.dll_overrides = "d3d11=n,b".into();
        o.validate().unwrap();
        for (loc, ok) in [
            ("ja_JP.UTF-8", true),
            ("C", true),
            ("de_DE@euro", true),
            ("en_US.utf8@x", true),
            ("", false),
            ("ja-JP", false),
            ("en_US.", false),
            ("en_US@", false),
            ("en US", false),
        ] {
            assert_eq!(is_valid_locale(loc), ok, "{loc}");
        }
        o.wine_debug = Some("+relay,-heap".into());
        o.validate().unwrap();
        o.wine_debug = Some("$(rm)".into());
        assert!(o.validate().is_err());
    }

    #[test]
    fn tool_rules() {
        assert!(is_valid_winetricks_verb("vcrun2019"));
        assert!(is_valid_winetricks_verb("win10"));
        assert!(is_valid_winetricks_verb("sound=alsa"));
        assert!(!is_valid_winetricks_verb("bad verb"));
        assert!(!is_valid_winetricks_verb("=x"));
        assert!(!is_valid_winetricks_verb("a=b=c"));
        assert!(Tool::Winetricks { verbs: vec![] }.validate().is_err());
        assert!(
            Tool::RunExe {
                path: "rel.exe".into(),
                args: vec![]
            }
            .validate()
            .is_err()
        );
        Tool::Winecfg.validate().unwrap();
    }

    #[test]
    fn serde_shapes() {
        let c = LaunchConfig {
            working_dir: WorkingDir::Custom("/games/x".into()),
            compat: CompatOptions {
                renderer: Renderer::OpenGl,
                sync: SyncMode::NoEsyncNoFsync,
                ..CompatOptions::default()
            },
            ..LaunchConfig::default()
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["working_dir"], serde_json::json!({"custom": "/games/x"}));
        assert_eq!(v["compat"]["renderer"], "opengl");
        assert_eq!(v["compat"]["sync"], "no_esync_no_fsync");
        assert_eq!(v["compat"]["window_system"], "auto");
        let back: LaunchConfig = serde_json::from_value(v).unwrap();
        assert_eq!(back, c);
        let t = Tool::Winetricks {
            verbs: vec!["vcrun2019".into()],
        };
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            serde_json::json!({"winetricks": {"verbs": ["vcrun2019"]}})
        );
        assert_eq!(
            serde_json::to_value(Tool::ControlPanel).unwrap(),
            "control_panel"
        );
    }
}
