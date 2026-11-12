//! Classification of user-defined variables (UMU_INTEGRATION §4 table D) and the never-set
//! list.
//!
//! `sandbox` must not depend on `umu` (MODULES §5); this is the planner's copy of the table
//! rules, kept in sync with `umu::env` by the table tests.

use serde::{Deserialize, Serialize};

/// Rule for a user-supplied variable name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserVarRule {
    /// Set as given.
    Allowed,
    /// Controlled by EasyPlay; cannot be set.
    Reserved,
    /// Developer Mode only.
    Dangerous,
    /// Not a valid name (`^[A-Za-z_][A-Za-z0-9_]{0,127}$`).
    Invalid,
}

const RESERVED_EXACT: &[&str] = &[
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

const RESERVED_PREFIX: &[&str] = &[
    "DBUS_",
    "UMU_",
    "PRESSURE_VESSEL_",
    "STEAM_COMPAT_",
    "STEAM_RUNTIME",
    "EASYPLAY_",
];

const DANGEROUS_EXACT: &[&str] = &[
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

const DANGEROUS_PREFIX: &[&str] = &["GST_PLUGIN_"];

/// Names never present in a sandbox environment, whatever the source (UMU_INTEGRATION §4).
const NEVER_SET: &[&str] = &[
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
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && name.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Classify a user-supplied variable name.
pub fn classify_user_var(name: &str) -> UserVarRule {
    if !valid_name(name) {
        UserVarRule::Invalid
    } else if RESERVED_EXACT.contains(&name) || RESERVED_PREFIX.iter().any(|p| name.starts_with(p))
    {
        UserVarRule::Reserved
    } else if DANGEROUS_EXACT.contains(&name)
        || DANGEROUS_PREFIX.iter().any(|p| name.starts_with(p))
    {
        UserVarRule::Dangerous
    } else {
        UserVarRule::Allowed
    }
}

/// Whether `name` must never appear in the sandbox environment whatever the source
/// (`STEAM_COMPAT_*` is set by umu itself). `LD_PRELOAD` and `LD_LIBRARY_PATH` are not on
/// this list: EasyPlay never sets them, but Developer Mode user variables may (table D).
pub fn is_never_set(name: &str) -> bool {
    NEVER_SET.contains(&name) || name.starts_with("STEAM_COMPAT_")
}
