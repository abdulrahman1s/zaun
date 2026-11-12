//! The umu and Proton variable tables (UMU_INTEGRATION §4) as data, the classification of
//! user-supplied variables (table D), value validation, the compatibility-option mapping
//! (table C) and the never-set list.
//!
//! This module is the single source of these rules: `sandbox::validate` re-exports
//! [`classify`] and [`is_never_set`] (as `classify_user_var`), and the sandbox planner's
//! environment section is checked against [`compat_vars`] and [`HOST_PASSTHROUGH`] by the
//! table tests.

use serde::{Deserialize, Serialize};

use crate::consts::{MAX_ENV_NAME_BYTES, MAX_ENV_VALUE_BYTES};
use crate::model::{CompatOptions, EnvVar, Renderer, SessionKind, SyncMode, WindowSystem};

/// Rule for a user-supplied variable name (table D).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvRule {
    /// Set as given.
    Allowed,
    /// Controlled by EasyPlay; cannot be set.
    Reserved,
    /// Developer Mode only.
    Dangerous,
    /// Not a valid name (`^[A-Za-z_][A-Za-z0-9_]{0,127}$`).
    Invalid,
}

impl EnvRule {
    /// Whether a variable with this rule may be applied.
    pub const fn permits(self, developer_mode: bool) -> bool {
        match self {
            EnvRule::Allowed => true,
            EnvRule::Dangerous => developer_mode,
            EnvRule::Reserved | EnvRule::Invalid => false,
        }
    }
}

// ---- table A ------------------------------------------------------------------------------

/// Which session kinds a fixed variable applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    /// Every kind.
    All,
    /// Every kind except Provision.
    AllButProvision,
    /// Only when the condition in the note holds (socket bound, option on, Isolated mode).
    Conditional,
    /// Never set by EasyPlay (left to the runtime).
    Never,
}

/// One row of table A.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedVar {
    /// Variable name.
    pub name: &'static str,
    /// When it is set.
    pub applies: Applies,
    /// Value or source, as documented.
    pub value: &'static str,
}

const fn fixed(name: &'static str, applies: Applies, value: &'static str) -> FixedVar {
    FixedVar {
        name,
        applies,
        value,
    }
}

/// Table A: variables EasyPlay sets itself.
pub const TABLE_A: &[FixedVar] = &[
    fixed("HOME", Applies::All, "real home path"),
    fixed("USER", Applies::All, "user name"),
    fixed("LOGNAME", Applies::All, "user name"),
    fixed(
        "PATH",
        Applies::All,
        "layout PATH plus the directory of umu-run",
    ),
    fixed("XDG_RUNTIME_DIR", Applies::All, "/run/user/<uid>"),
    fixed(
        "WINEPREFIX",
        Applies::All,
        "prefix, or /tmp/easyplay-prefix",
    ),
    fixed(
        "PROTONPATH",
        Applies::All,
        "absolute build path or codename",
    ),
    fixed("GAMEID", Applies::All, "umu ID, default umu-default"),
    fixed("STORE", Applies::Conditional, "store identifier if set"),
    fixed("UMU_RUNTIME_UPDATE", Applies::AllButProvision, "0"),
    fixed(
        "UMU_LOG",
        Applies::Conditional,
        "1 with verbose launch logging",
    ),
    fixed(
        "PRESSURE_VESSEL_VARIABLE_DIR",
        Applies::Conditional,
        "/run/easyplay/pv-var (Isolated mode)",
    ),
    fixed("PRESSURE_VESSEL_SHARE_HOME", Applies::All, "1"),
    fixed("GST_DEBUG", Applies::All, "0"),
    fixed(
        "DISPLAY",
        Applies::Conditional,
        "when the X11 socket is bound",
    ),
    fixed(
        "XAUTHORITY",
        Applies::Conditional,
        "when the X11 socket is bound",
    ),
    fixed(
        "WAYLAND_DISPLAY",
        Applies::Conditional,
        "when the Wayland socket is bound",
    ),
    fixed(
        "PULSE_SERVER",
        Applies::Conditional,
        "when the PulseAudio socket is bound",
    ),
    fixed("PROTON_LOG", Applies::Conditional, "1 with the Proton log"),
    fixed(
        "PROTON_LOG_DIR",
        Applies::Conditional,
        "/run/easyplay/proton-log with the Proton log",
    ),
    fixed("SDL_VIDEODRIVER", Applies::Never, "left to the runtime"),
    fixed("QT_QPA_PLATFORM", Applies::Never, "left to the runtime"),
    fixed("GDK_BACKEND", Applies::Never, "left to the runtime"),
];

/// `GAMEID` when the game has no umu ID.
pub const DEFAULT_GAME_ID: &str = "umu-default";

/// `UMU_RUNTIME_UPDATE` value for every kind but Provision (F-123, F-226).
pub const RUNTIME_UPDATE_OFF: &str = "0";

/// Whether `UMU_RUNTIME_UPDATE=0` is set for `kind`.
pub const fn disables_runtime_update(kind: SessionKind) -> bool {
    !matches!(kind, SessionKind::Provision)
}

// ---- table B ------------------------------------------------------------------------------

/// Table B: host variables copied when set, every group but the layout-specific one (which
/// is data in `host::layout::NIX_PASSTHROUGH_ENV` and lands in `FsLayout::passthrough_env`).
pub const HOST_PASSTHROUGH: &[&str] = &[
    // Locale and time.
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "TZ",
    // Session description.
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
    "DESKTOP_SESSION",
    // Cursor.
    "XCURSOR_THEME",
    "XCURSOR_SIZE",
    // GPU selection.
    "DRI_PRIME",
    "MESA_VK_DEVICE_SELECT",
    "__NV_PRIME_RENDER_OFFLOAD",
    "__NV_PRIME_RENDER_OFFLOAD_PROVIDER",
    "__GLX_VENDOR_LIBRARY_NAME",
    "__VK_LAYER_NV_optimus",
];

/// Tuning variables that are never forwarded from the launcher's environment (they come from
/// tables C and D only, §4 last paragraph). Informational: the planner copies only the
/// table B allow-list anyway.
pub const NOT_FORWARDED: &[&str] = &[
    "PROTON_LOG",
    "PROTON_ENABLE_WAYLAND",
    "PROTON_USE_WINED3D",
    "PROTON_NO_ESYNC",
    "PROTON_NO_FSYNC",
    "DXVK_HUD",
    "DXVK_CONFIG_FILE",
    "VKD3D_CONFIG",
    "WINEDLLOVERRIDES",
    "UMU_LOG",
];

// ---- table C ------------------------------------------------------------------------------

/// One variable produced by a compatibility option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatVar {
    /// Variable name.
    pub name: &'static str,
    /// Value.
    pub value: String,
    /// The `CompatOptions` field it comes from (the `option:<name>` origin).
    pub option: &'static str,
}

/// Facts the mapping depends on besides the options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompatContext {
    /// The Wayland socket is bound (native Wayland is only requested then).
    pub wayland: bool,
    /// Developer Mode (Wine debug channels).
    pub developer_mode: bool,
}

/// `PROTON_LOG_DIR` inside the sandbox (BUBBLEWRAP_SANDBOX §4.7).
pub const PROTON_LOG_DIR: &str = "/run/easyplay/proton-log";

/// Table C: the variables a game's compatibility options produce, in table order.
pub fn compat_vars(c: &CompatOptions, cx: CompatContext) -> Vec<CompatVar> {
    let mut v = Vec::new();
    let mut push = |name, value: &str, option| {
        v.push(CompatVar {
            name,
            value: value.to_owned(),
            option,
        });
    };
    if c.renderer == Renderer::OpenGl {
        push("PROTON_USE_WINED3D", "1", "renderer");
    }
    if c.window_system == WindowSystem::Wayland && cx.wayland {
        push("PROTON_ENABLE_WAYLAND", "1", "window_system");
    }
    match c.sync {
        SyncMode::Default => {}
        SyncMode::NoFsync => push("PROTON_NO_FSYNC", "1", "sync"),
        SyncMode::NoEsyncNoFsync => {
            push("PROTON_NO_FSYNC", "1", "sync");
            push("PROTON_NO_ESYNC", "1", "sync");
        }
    }
    if c.disable_fixes {
        push("PROTONFIXES_DISABLE", "1", "disable_fixes");
    }
    if !c.dll_overrides.is_empty() {
        push("WINEDLLOVERRIDES", &c.dll_overrides, "dll_overrides");
    }
    if c.mangohud {
        push("MANGOHUD", "1", "mangohud");
    }
    if let Some(hud) = &c.dxvk_hud {
        push("DXVK_HUD", hud, "dxvk_hud");
    }
    if let Some(locale) = &c.locale {
        push("LC_ALL", locale, "locale");
    }
    if c.proton_log {
        push("PROTON_LOG", "1", "proton_log");
        push("PROTON_LOG_DIR", PROTON_LOG_DIR, "proton_log");
    }
    if let Some(debug) = &c.wine_debug
        && cx.developer_mode
    {
        push("WINEDEBUG", debug, "wine_debug");
    }
    v
}

// ---- table D ------------------------------------------------------------------------------

/// Names reserved exactly.
pub const RESERVED_EXACT: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "PATH",
    "PWD",
    "SHELL",
    "XDG_RUNTIME_DIR",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "PULSE_SERVER",
    "WINEPREFIX",
    "PROTONPATH",
    "GAMEID",
    "STORE",
    "SteamAppId",
    "SteamGameId",
    "container",
];

/// Reserved name prefixes (`DBUS_*`, `UMU_*`, …; `STEAM_RUNTIME*` has no underscore).
pub const RESERVED_PREFIX: &[&str] = &[
    "DBUS_",
    "UMU_",
    "PRESSURE_VESSEL_",
    "STEAM_COMPAT_",
    "STEAM_RUNTIME",
    "EASYPLAY_",
];

/// Names allowed only in Developer Mode.
pub const DANGEROUS_EXACT: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "LD_DEBUG",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "WINESERVER",
    "WINELOADER",
    "WINEDLLPATH",
    "VK_ICD_FILENAMES",
    "VK_DRIVER_FILES",
    "VK_LAYER_PATH",
    "VK_ADD_LAYER_PATH",
    "LIBGL_DRIVERS_PATH",
    "__EGL_VENDOR_LIBRARY_FILENAMES",
];

/// Dangerous name prefixes.
pub const DANGEROUS_PREFIX: &[&str] = &["GST_PLUGIN_"];

/// Names never present in a sandbox environment, whatever the source (§4 "Never set").
/// `STEAM_COMPAT_*` is matched as a prefix. `LD_PRELOAD` and `LD_LIBRARY_PATH` are not on
/// this list: EasyPlay never sets them itself (except the layout-specific table B copy of
/// `LD_LIBRARY_PATH`), but Developer Mode user variables may.
pub const NEVER_SET: &[&str] = &[
    "container",
    "UMU_ZENITY",
    "UMU_NO_PROTON",
    "UMU_CONTAINER_NSENTER",
    "UMU_FOLDERS_PATH",
    "DBUS_SESSION_BUS_ADDRESS",
    "DBUS_SYSTEM_BUS_ADDRESS",
    "SteamAppId",
    "SteamGameId",
];

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(c) if c.is_ascii_alphabetic() || c == b'_')
        && name.len() <= MAX_ENV_NAME_BYTES
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Classify a user-supplied variable name (table D).
pub fn classify(name: &str) -> EnvRule {
    if !valid_name(name) {
        EnvRule::Invalid
    } else if RESERVED_EXACT.contains(&name) || RESERVED_PREFIX.iter().any(|p| name.starts_with(p))
    {
        EnvRule::Reserved
    } else if DANGEROUS_EXACT.contains(&name)
        || DANGEROUS_PREFIX.iter().any(|p| name.starts_with(p))
    {
        EnvRule::Dangerous
    } else {
        EnvRule::Allowed
    }
}

/// Whether `name` must never appear in the sandbox environment whatever the source.
pub fn is_never_set(name: &str) -> bool {
    NEVER_SET.contains(&name) || name.starts_with("STEAM_COMPAT_")
}

/// Why a value is rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValueProblem {
    /// Longer than [`MAX_ENV_VALUE_BYTES`].
    #[error("the value is longer than {MAX_ENV_VALUE_BYTES} bytes")]
    TooLong,
    /// Contains a NUL byte.
    #[error("the value contains a NUL byte")]
    ContainsNul,
}

/// Validate a variable value (≤ 4 096 bytes, no NUL).
pub fn validate_value(value: &str) -> Result<(), ValueProblem> {
    if value.len() > MAX_ENV_VALUE_BYTES {
        Err(ValueProblem::TooLong)
    } else if value.contains('\0') {
        Err(ValueProblem::ContainsNul)
    } else {
        Ok(())
    }
}

/// Why a user variable cannot be applied.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UserVarError {
    /// The name is not valid.
    #[error("{0}: not a valid variable name")]
    InvalidName(String),
    /// The name is controlled by EasyPlay.
    #[error("{0}: set by EasyPlay")]
    Reserved(String),
    /// The name needs Developer Mode.
    #[error("{0}: needs Developer Mode")]
    NeedsDeveloperMode(String),
    /// The value is invalid.
    #[error("{name}: {problem}")]
    Value {
        /// Variable name.
        name: String,
        /// What is wrong.
        problem: ValueProblem,
    },
}

/// Check a user variable completely (name rule and value), as the editor and the planner do.
pub fn check_user_var(var: &EnvVar, developer_mode: bool) -> Result<(), UserVarError> {
    match classify(&var.name) {
        EnvRule::Invalid => return Err(UserVarError::InvalidName(var.name.clone())),
        EnvRule::Reserved => return Err(UserVarError::Reserved(var.name.clone())),
        EnvRule::Dangerous if !developer_mode => {
            return Err(UserVarError::NeedsDeveloperMode(var.name.clone()));
        }
        EnvRule::Dangerous | EnvRule::Allowed => {}
    }
    validate_value(&var.value).map_err(|problem| UserVarError::Value {
        name: var.name.clone(),
        problem,
    })
}

/// The setting that controls a reserved variable, for the editor's explanation (message key
/// suffix).
pub fn reserved_hint(name: &str) -> Option<&'static str> {
    if classify(name) != EnvRule::Reserved {
        return None;
    }
    Some(match name {
        "WINEPREFIX" => "prefix",
        "PROTONPATH" => "compatibility_layer",
        "GAMEID" | "STORE" => "game_fixes",
        "DISPLAY" | "WAYLAND_DISPLAY" | "XAUTHORITY" => "display",
        "PULSE_SERVER" => "audio",
        n if n.starts_with("UMU_") || n.starts_with("PRESSURE_VESSEL_") => "launcher",
        n if n.starts_with("DBUS_") => "session_bus",
        _ => "sandbox",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name of table D with its rule.
    #[test]
    fn table_d_rows() {
        for n in RESERVED_EXACT {
            assert_eq!(classify(n), EnvRule::Reserved, "{n}");
        }
        for (prefix, example) in [
            ("DBUS_", "DBUS_SESSION_BUS_ADDRESS"),
            ("UMU_", "UMU_ZENITY"),
            ("PRESSURE_VESSEL_", "PRESSURE_VESSEL_FILESYSTEMS_RO"),
            ("STEAM_COMPAT_", "STEAM_COMPAT_DATA_PATH"),
            ("STEAM_RUNTIME", "STEAM_RUNTIME_LIBRARY_PATH"),
            ("EASYPLAY_", "EASYPLAY_LOG"),
        ] {
            assert!(RESERVED_PREFIX.contains(&prefix));
            assert_eq!(classify(example), EnvRule::Reserved, "{example}");
            assert_eq!(classify(prefix), EnvRule::Reserved, "{prefix}");
        }
        assert_eq!(classify("STEAM_RUNTIME"), EnvRule::Reserved);
        for n in DANGEROUS_EXACT {
            assert_eq!(classify(n), EnvRule::Dangerous, "{n}");
        }
        assert_eq!(classify("GST_PLUGIN_PATH"), EnvRule::Dangerous);
        assert_eq!(classify("GST_PLUGIN_SYSTEM_PATH_1_0"), EnvRule::Dangerous);
        for n in [
            "DXVK_HUD",
            "MY_VAR",
            "_x",
            "a",
            "PROTON_LOG",
            "WINEDEBUG",
            "GST_DEBUG",
        ] {
            assert_eq!(classify(n), EnvRule::Allowed, "{n}");
        }
    }

    #[test]
    fn syntax() {
        let long_ok = "A".repeat(128);
        let long_bad = "A".repeat(129);
        for (n, rule) in [
            ("", EnvRule::Invalid),
            ("1ABC", EnvRule::Invalid),
            ("A-B", EnvRule::Invalid),
            ("A B", EnvRule::Invalid),
            ("Ä", EnvRule::Invalid),
            ("A=B", EnvRule::Invalid),
            (long_ok.as_str(), EnvRule::Allowed),
            (long_bad.as_str(), EnvRule::Invalid),
            ("_9", EnvRule::Allowed),
        ] {
            assert_eq!(classify(n), rule, "{n:?}");
        }
    }

    #[test]
    fn values() {
        assert_eq!(validate_value(""), Ok(()));
        assert_eq!(validate_value(&"x".repeat(4096)), Ok(()));
        assert_eq!(
            validate_value(&"x".repeat(4097)),
            Err(ValueProblem::TooLong)
        );
        assert_eq!(validate_value("a\0b"), Err(ValueProblem::ContainsNul));
        let var = |n: &str, v: &str| EnvVar {
            name: n.into(),
            value: v.into(),
        };
        assert!(check_user_var(&var("MY", "1"), false).is_ok());
        assert!(matches!(
            check_user_var(&var("LD_PRELOAD", "x"), false),
            Err(UserVarError::NeedsDeveloperMode(_))
        ));
        assert!(check_user_var(&var("LD_PRELOAD", "x"), true).is_ok());
        assert!(matches!(
            check_user_var(&var("HOME", "x"), true),
            Err(UserVarError::Reserved(_))
        ));
        assert!(matches!(
            check_user_var(&var("A", "\0"), true),
            Err(UserVarError::Value { .. })
        ));
        assert!(matches!(
            check_user_var(&var("9", ""), true),
            Err(UserVarError::InvalidName(_))
        ));
    }

    /// Every table A name is either reserved for user input or deliberately overridable.
    #[test]
    fn table_a_rows() {
        let user_settable = [
            "GST_DEBUG",
            "PROTON_LOG",
            "PROTON_LOG_DIR",
            "SDL_VIDEODRIVER",
            "QT_QPA_PLATFORM",
            "GDK_BACKEND",
        ];
        for row in TABLE_A {
            let expected = if user_settable.contains(&row.name) {
                EnvRule::Allowed
            } else {
                EnvRule::Reserved
            };
            assert_eq!(classify(row.name), expected, "{}", row.name);
            assert!(!is_never_set(row.name), "{}", row.name);
        }
        assert!(disables_runtime_update(SessionKind::Game));
        assert!(disables_runtime_update(SessionKind::Prepare));
        assert!(!disables_runtime_update(SessionKind::Provision));
    }

    /// Table B names are user-settable (a per-game value overrides the host copy) and none is
    /// on the never-set list.
    #[test]
    fn table_b_rows() {
        for n in HOST_PASSTHROUGH {
            assert_eq!(classify(n), EnvRule::Allowed, "{n}");
            assert!(!is_never_set(n));
        }
        for n in crate::host::layout::NIX_PASSTHROUGH_ENV {
            assert_ne!(classify(n), EnvRule::Invalid, "{n}");
            assert!(!is_never_set(n));
        }
        for n in NOT_FORWARDED {
            assert!(!HOST_PASSTHROUGH.contains(n));
        }
    }

    #[test]
    fn table_c_rows() {
        let cx = CompatContext {
            wayland: true,
            developer_mode: true,
        };
        let names = |c: &CompatOptions, cx| -> Vec<(&str, String)> {
            compat_vars(c, cx)
                .into_iter()
                .map(|v| (v.name, v.value))
                .collect()
        };
        let base = CompatOptions::default();
        assert!(names(&base, cx).is_empty());
        let one = |f: &dyn Fn(&mut CompatOptions)| {
            let mut c = base.clone();
            f(&mut c);
            names(&c, cx)
        };
        let v = |pairs: &[(&'static str, &str)]| -> Vec<(&'static str, String)> {
            pairs.iter().map(|(n, x)| (*n, (*x).to_owned())).collect()
        };
        assert_eq!(
            one(&|c| c.renderer = Renderer::OpenGl),
            v(&[("PROTON_USE_WINED3D", "1")])
        );
        assert_eq!(
            one(&|c| c.window_system = WindowSystem::Wayland),
            v(&[("PROTON_ENABLE_WAYLAND", "1")])
        );
        assert_eq!(one(&|c| c.window_system = WindowSystem::X11), v(&[]));
        assert_eq!(
            one(&|c| c.sync = SyncMode::NoFsync),
            v(&[("PROTON_NO_FSYNC", "1")])
        );
        assert_eq!(
            one(&|c| c.sync = SyncMode::NoEsyncNoFsync),
            v(&[("PROTON_NO_FSYNC", "1"), ("PROTON_NO_ESYNC", "1")])
        );
        assert_eq!(
            one(&|c| c.disable_fixes = true),
            v(&[("PROTONFIXES_DISABLE", "1")])
        );
        assert_eq!(
            one(&|c| c.dll_overrides = "d3d11=n,b".into()),
            v(&[("WINEDLLOVERRIDES", "d3d11=n,b")])
        );
        assert_eq!(one(&|c| c.mangohud = true), v(&[("MANGOHUD", "1")]));
        assert_eq!(
            one(&|c| c.dxvk_hud = Some("fps".into())),
            v(&[("DXVK_HUD", "fps")])
        );
        assert_eq!(
            one(&|c| c.locale = Some("ja_JP.UTF-8".into())),
            v(&[("LC_ALL", "ja_JP.UTF-8")])
        );
        assert_eq!(
            one(&|c| c.proton_log = true),
            v(&[("PROTON_LOG", "1"), ("PROTON_LOG_DIR", PROTON_LOG_DIR)])
        );
        assert_eq!(
            one(&|c| c.wine_debug = Some("+seh".into())),
            v(&[("WINEDEBUG", "+seh")])
        );
        // Conditions.
        let mut c = base.clone();
        c.window_system = WindowSystem::Wayland;
        c.wine_debug = Some("+seh".into());
        assert!(names(&c, CompatContext::default()).is_empty());
    }

    #[test]
    fn never_set_and_hints() {
        for n in NEVER_SET {
            assert!(is_never_set(n));
        }
        assert!(is_never_set("STEAM_COMPAT_MOUNTS"));
        assert!(!is_never_set("LD_PRELOAD"));
        assert_eq!(reserved_hint("PROTONPATH"), Some("compatibility_layer"));
        assert_eq!(reserved_hint("UMU_LOG"), Some("launcher"));
        assert_eq!(reserved_hint("MY_VAR"), None);
        assert!(EnvRule::Dangerous.permits(true));
        assert!(!EnvRule::Dangerous.permits(false));
        assert!(!EnvRule::Reserved.permits(true));
    }
}
