//! Wording of service notices and of the real backend's stand-ins.

/// A command this build of the service does not implement yet.
pub const NOT_AVAILABLE: &str = "This isn't available in this version of EasyPlay yet.";
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

/// "Deleted all play history" / "Deleted the play history of Hollow Knight".
pub fn stats_cleared(name: Option<&str>, sessions: u64) -> String {
    let n = format!(
        "{sessions} {}",
        super::plural(sessions, "session", "sessions")
    );
    match name {
        Some(g) => format!("Deleted the play history of {g} ({n})"),
        None => format!("Deleted all play history ({n})"),
    }
}

/// Notice.
pub const SESSION_REMOVED: &str = "Removed the session from your history";

/// "Added a program to Hollow Knight".
pub fn program_added(name: &str) -> String {
    format!("Added a program to {name}")
}

/// "Found the program of Hollow Knight".
pub fn program_located(name: &str) -> String {
    format!("Found the program of {name}")
}

/// "Started the tool for Hollow Knight".
pub fn tool_started(name: &str) -> String {
    format!("Started the tool for {name}")
}

/// "Cancelled starting Hollow Knight".
pub fn launch_cancelled(name: &str) -> String {
    format!("Cancelled starting {name}")
}

/// "Report saved as easyplay-report.tar.gz".
pub fn report_saved(file: &str) -> String {
    format!("Report saved as {file}")
}

/// The library was repaired, restored or replaced.
pub fn library_recovered(
    how: easyplay_core::service::RecoveredHow,
    history_rows_lost: Option<u64>,
) -> String {
    use easyplay_core::service::RecoveredHow as H;
    let base = match how {
        H::Repaired => "Your library was repaired",
        H::Restored => "Your library was restored from the backup",
        H::UsedRepaired => "Your library now uses what could be recovered",
        H::StartedEmpty => "A new, empty library was started; the damaged one was kept",
    };
    match history_rows_lost {
        Some(n) if n > 0 => format!("{base} ({n} play history entries could not be recovered)"),
        _ => base.to_owned(),
    }
}

/// The Doctor row name of a check ID (ERROR_HANDLING §6).
pub fn doctor_check_name(id: &str) -> &'static str {
    match id {
        "D-01" => "Session folder",
        "D-02" => "EasyPlay's folders",
        "D-03" => "Library database",
        "D-04" => "Sandbox tool",
        "D-05" => "User namespaces",
        "D-06" => "Nested sandboxes",
        "D-07" => "Protected game folders",
        "D-08" => "Private runtime copies",
        "D-09" => "Compatibility launcher",
        "D-10" => "Sandbox helper",
        "D-11" => "Display",
        "D-12" => "Graphics",
        "D-13" => "Sound",
        "D-14" => "Controllers",
        "D-15" => "Compatibility layers",
        "D-16" => "Default application",
        "D-17" => "File systems",
        _ => "Check",
    }
}
