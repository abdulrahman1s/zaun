//! Wording of service notices and of the real backend's stand-ins.

/// A command this build of the service does not implement yet.
pub const NOT_AVAILABLE: &str = "This isn't available in this version of EasyPlay yet.";
/// Folder scanning is not implemented by the service yet.
pub const SCAN_NOT_AVAILABLE: &str =
    "Listing the programs in a folder isn't available yet. Choose a file instead.";
/// Notice.
pub const KEPT_PLACEHOLDER: &str = "The program's icon could not be used";
/// Check row.
pub const CHECK_SANDBOX: &str = "Sandbox tool";
/// Check row.
pub const CHECK_LAUNCHER: &str = "Compatibility launcher";
/// Check result.
pub const NOT_FOUND: &str = "Not found";

/// "Hollow Knight is already in your library".
pub fn already(name: &str) -> String {
    if name.is_empty() {
        "Already in your library".to_owned()
    } else {
        format!("{name} is already in your library")
    }
}

/// "Found Hollow Knight in its new location".
pub fn found_moved(name: &str) -> String {
    format!("Found {name} in its new location")
}

/// "Only the first 50 files were opened".
pub fn only_first(n: usize) -> String {
    format!("Only the first {n} files were opened")
}

/// "Artwork updated for Hollow Knight".
pub fn artwork(name: &str) -> String {
    format!("Artwork updated for {name}")
}

/// "Steam Linux Runtime (steamrt3) copied into EasyPlay".
pub fn adopted(variant: &str) -> String {
    format!("Steam Linux Runtime ({variant}) copied into EasyPlay")
}

/// "bubblewrap 0.11.0".
pub fn version(tool: &str, v: Option<String>) -> String {
    match v {
        Some(v) => format!("{tool} {v}"),
        None => tool.to_owned(),
    }
}

/// "sudo sysctl -w kernel.unprivileged_userns_clone=1".
pub fn sysctl(setting: &str) -> String {
    format!("sudo sysctl -w {setting}")
}
