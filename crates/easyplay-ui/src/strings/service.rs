//! Wording of service notices and of the real backend's stand-ins.

texts! {
    /// Notice.
    KEPT_PLACEHOLDER = "service.kept_placeholder";
    /// Check row.
    CHECK_SANDBOX = "service.check_sandbox";
    /// Check row.
    CHECK_LAUNCHER = "service.check_launcher";
    /// Check result.
    NOT_FOUND = "service.not_found";
}

/// "Hollow Knight is already in your library".
pub fn already(name: &str) -> String {
    if name.is_empty() {
        crate::i18n::text("service.already_unnamed").to_owned()
    } else {
        rust_i18n::t!("service.already", name = name).into_owned()
    }
}

/// "Found Hollow Knight in its new location".
pub fn found_moved(name: &str) -> String {
    rust_i18n::t!("service.found_moved", name = name).into_owned()
}

/// "Only the first 50 files were opened".
pub fn only_first(n: usize) -> String {
    rust_i18n::t!("service.only_first", n = n).into_owned()
}

/// "Artwork updated for Hollow Knight".
pub fn artwork(name: &str) -> String {
    rust_i18n::t!("service.artwork", name = name).into_owned()
}

/// "Steam Linux Runtime (steamrt3) copied into EasyPlay".
pub fn adopted(variant: &str) -> String {
    rust_i18n::t!("service.adopted", variant = variant).into_owned()
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
    let count = crate::i18n::plural::counted("service.sessions", sessions);
    match name {
        Some(g) => {
            rust_i18n::t!("service.stats_cleared_game", name = g, count = count).into_owned()
        }
        None => rust_i18n::t!("service.stats_cleared_all", count = count).into_owned(),
    }
}

texts! {
    /// Notice.
    SESSION_REMOVED = "service.session_removed";
}

/// "Added a program to Hollow Knight".
pub fn program_added(name: &str) -> String {
    rust_i18n::t!("service.program_added", name = name).into_owned()
}

/// "Found the program of Hollow Knight".
pub fn program_located(name: &str) -> String {
    rust_i18n::t!("service.program_located", name = name).into_owned()
}

/// "Started the tool for Hollow Knight".
pub fn tool_started(name: &str) -> String {
    rust_i18n::t!("service.tool_started", name = name).into_owned()
}

/// "Cancelled starting Hollow Knight".
pub fn launch_cancelled(name: &str) -> String {
    rust_i18n::t!("service.launch_cancelled", name = name).into_owned()
}

/// "Report saved as easyplay-report.tar.gz".
pub fn report_saved(file: &str) -> String {
    rust_i18n::t!("service.report_saved", file = file).into_owned()
}

/// The library was repaired, restored or replaced.
pub fn library_recovered(
    how: easyplay_core::service::RecoveredHow,
    history_rows_lost: Option<u64>,
) -> String {
    use easyplay_core::service::RecoveredHow as H;
    let base = match how {
        H::Repaired => crate::i18n::text("service.recovered.repaired"),
        H::Restored => crate::i18n::text("service.recovered.restored"),
        H::UsedRepaired => crate::i18n::text("service.recovered.used_repaired"),
        H::StartedEmpty => crate::i18n::text("service.recovered.started_empty"),
    };
    match history_rows_lost {
        Some(n) if n > 0 => {
            crate::i18n::plural::counted_args("service.recovered_lost", n, &[("base", base)])
        }
        _ => base.to_owned(),
    }
}

/// The Doctor row name of a check ID (ERROR_HANDLING §6).
pub fn doctor_check_name(id: &str) -> &'static str {
    match id {
        "D-01" => crate::i18n::text("service.doctor_check_name.session_folder"),
        "D-02" => crate::i18n::text("service.doctor_check_name.easyplay_s_folders"),
        "D-03" => crate::i18n::text("service.doctor_check_name.library_database"),
        "D-04" => crate::i18n::text("service.doctor_check_name.sandbox_tool"),
        "D-05" => crate::i18n::text("service.doctor_check_name.user_namespaces"),
        "D-06" => crate::i18n::text("service.doctor_check_name.nested_sandboxes"),
        "D-07" => crate::i18n::text("service.doctor_check_name.protected_game_folders"),
        "D-08" => crate::i18n::text("service.doctor_check_name.private_runtime_copies"),
        "D-09" => crate::i18n::text("service.doctor_check_name.compatibility_launcher"),
        "D-10" => crate::i18n::text("service.doctor_check_name.sandbox_helper"),
        "D-11" => crate::i18n::text("service.doctor_check_name.display"),
        "D-12" => crate::i18n::text("service.doctor_check_name.graphics"),
        "D-13" => crate::i18n::text("service.doctor_check_name.sound"),
        "D-14" => crate::i18n::text("service.doctor_check_name.controllers"),
        "D-15" => crate::i18n::text("service.doctor_check_name.compatibility_layers"),
        "D-16" => crate::i18n::text("service.doctor_check_name.default_application"),
        "D-17" => crate::i18n::text("service.doctor_check_name.file_systems"),
        _ => crate::i18n::text("service.doctor_check_name.check"),
    }
}
