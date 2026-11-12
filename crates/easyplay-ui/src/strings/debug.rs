//! Developer Mode views, the diagnostics report, the Doctor and recovery mode
//! (DEVELOPER_MODE, SCREEN_SPECS_2 §15–16, ERROR_HANDLING §4.11–4.13, §5–7).

/// Inspector sheet title.
pub const INSPECTOR: &str = "Launch inspector";
/// Inspector title for the dry run.
pub const INSPECTOR_NEXT: &str = "Launch inspector — next launch";
/// Tab names, in order.
pub const INSPECTOR_TABS: [&str; 8] = [
    "Summary",
    "Access",
    "Command",
    "Sandbox",
    "Environment",
    "Processes",
    "Events",
    "Raw",
];
/// Accessible name of the tab row.
pub const INSPECTOR_TAB_LABEL: &str = "Inspector sections";
/// Loading.
pub const PLANNING: &str = "Planning the launch…";
/// Loading.
pub const LOADING: &str = "Loading…";
/// No plan for an old session.
pub const NOT_AVAILABLE_OLD: &str = "Not available for sessions created by an older version";
/// Dry-run label.
pub const DRY_RUN: &str = "Dry run: nothing was started, created or written.";
/// Summary keys.
pub const K_GAME: &str = "Game";
/// Summary key.
pub const K_KIND: &str = "Session kind";
/// Summary key.
pub const K_PRESET: &str = "Preset";
/// Summary key.
pub const K_NETWORK: &str = "Network";
/// Summary key.
pub const K_LAYER: &str = "Compatibility layer";
/// Summary key.
pub const K_ISOLATION: &str = "Isolation";
/// Summary key.
pub const K_PREFIX: &str = "Prefix";
/// Summary key.
pub const K_HOME: &str = "Home (in the sandbox)";
/// Summary key.
pub const K_WORKDIR: &str = "Working folder";
/// Summary key.
pub const K_SESSION: &str = "Session";
/// Summary key.
pub const K_END: &str = "End";
/// Summary key.
pub const K_TIMELINE: &str = "Timeline";
/// Notes heading.
pub const NOTES: &str = "Planner notes";
/// Access tab empty.
pub const NO_STATEMENTS: &str = "Access statements are shown for the next launch (dry run).";
/// Command tab.
pub const UMU_COMMAND: &str = "umu-run command";
/// Command tab.
pub const FULL_COMMAND: &str = "In-sandbox command (with the init shim)";
/// Copy as list.
pub const COPY_LIST: &str = "Copy as list";
/// Copy as shell command.
pub const COPY_SHELL: &str = "Copy as shell command";
/// What the shell copy contains.
pub const COPY_SHELL_NOTE: &str = "The script reproduces the launch by hand with the same \
     sandbox and system-call filter. It bypasses session tracking: no statistics, no prefix \
     lock. Review it before running it.";
/// Sandbox tab.
pub const NAMESPACES: &str = "Namespaces and lifecycle";
/// Sandbox tab.
pub const MOUNTS: &str = "Mounts";
/// Filter field.
pub const FILTER: &str = "Filter";
/// Toggle.
pub const ONLY_WRITABLE: &str = "Only writable";
/// Toggle.
pub const ONLY_CHOICES: &str = "Only from your choices";
/// Mount table headers.
pub const MOUNT_COLUMNS: [&str; 7] = [
    "#",
    "Kind",
    "Source",
    "Destination",
    "Mode",
    "Section",
    "Reason",
];
/// Sandbox tab.
pub const DESCRIPTORS: &str = "Descriptors";
/// Sandbox tab.
pub const SECCOMP_ON: &str = "System-call filter: on";
/// Sandbox tab.
pub const SECCOMP_OFF: &str = "System-call filter: off";
/// Environment headers.
pub const ENV_COLUMNS: [&str; 3] = ["Name", "Value", "Origin"];
/// Copy all.
pub const COPY_ALL: &str = "Copy all";
/// Copy one row.
pub const COPY: &str = "Copy";
/// umu's own variables.
pub const UMU_ADDS: &str = "Set by umu at run time (predictions): STEAM_COMPAT_DATA_PATH, \
     STEAM_COMPAT_CLIENT_INSTALL_PATH, STEAM_COMPAT_TOOL_PATHS, STEAM_COMPAT_LIBRARY_PATHS, \
     STEAM_COMPAT_INSTALL_PATH, STEAM_COMPAT_SHADER_PATH.";
/// Processes tab, past session.
pub const PROCESSES_PAST: &str = "Processes are shown while the session runs.";
/// Process headers.
pub const PROCESS_COLUMNS: [&str; 6] = ["PID", "Name", "Role", "State", "CPU time", "Memory"];
/// Stop session.
pub const STOP_SESSION: &str = "Stop session";
/// Events headers.
pub const EVENT_COLUMNS: [&str; 3] = ["Time", "Kind", "Details"];
/// No events.
pub const NO_EVENTS: &str = "No events recorded.";
/// Raw tab.
pub const RAW_ARGS: &str = "Arguments passed through --args";
/// Raw tab.
pub const RAW_JSON: &str = "Plan (JSON)";
/// Copy JSON.
pub const COPY_JSON: &str = "Copy JSON";
/// Copy NUL-separated.
pub const COPY_NUL: &str = "Copy NUL-separated";

/// "3 processes · 1 h 2 min CPU · 1.2 GB".
pub fn process_totals(n: usize, cpu_ms: u64, rss: u64) -> String {
    format!(
        "{n} {} · {} CPU · {}",
        super::plural(n as u64, "process", "processes"),
        cpu_time(cpu_ms),
        bytes(rss)
    )
}

/// CPU time as `m:ss.s` or `h:mm:ss`.
pub fn cpu_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}.{}", s / 60, s % 60, (ms % 1000) / 100)
    }
}

/// Bytes in KiB/MiB/GiB.
pub fn bytes(b: u64) -> String {
    const K: f64 = 1024.0;
    let f = b as f64;
    if f >= K * K * K {
        format!("{:.1} GiB", f / (K * K * K))
    } else if f >= K * K {
        format!("{:.0} MiB", f / (K * K))
    } else if b >= 1024 {
        format!("{:.0} KiB", f / K)
    } else {
        format!("{b} B")
    }
}

/// "+1.234 s".
pub fn offset_ms(ms: u64) -> String {
    format!("+{}.{:03} s", ms / 1000, ms % 1000)
}

// ---- logs ------------------------------------------------------------------------------------

/// Viewer title.
pub const LOGS: &str = "Logs";
/// Viewer title with the game.
pub fn logs_of(game: &str) -> String {
    format!("Log — {game}")
}
/// Session selector.
pub const SESSION: &str = "Session";
/// File selector names.
pub const LOG_FILES: [&str; 3] = ["game.log", "events.jsonl", "supervisor.log"];
/// File selector label.
pub const LOG_FILE: &str = "File";
/// Toggle.
pub const WRAP: &str = "Wrap";
/// Toggle.
pub const FOLLOW: &str = "Follow";
/// Open session folder.
pub const OPEN_FOLDER: &str = "Open session folder";
/// Copy visible.
pub const COPY_VISIBLE: &str = "Copy visible";
/// Open inspector.
pub const OPEN_INSPECTOR: &str = "Launch inspector";
/// No sessions.
pub const NO_SESSIONS: &str = "No sessions recorded yet for this game.";
/// Removed.
pub const LOG_REMOVED: &str = "This session's log was removed (logs are kept for 30 days).";
/// Empty log.
pub const LOG_EMPTY: &str = "The log is empty.";
/// Older lines dropped.
pub fn dropped(n: usize) -> String {
    format!("{n} older lines are not shown (at most 200,000 lines are kept)")
}
/// "3,412 lines · 12 match".
pub fn line_count(total: usize, shown: usize, filtering: bool) -> String {
    if filtering {
        format!("{shown} of {total} lines match")
    } else {
        format!("{total} {}", super::plural(total as u64, "line", "lines"))
    }
}
/// Session option label.
pub fn session_label(when: &str, kind: &str, end: &str) -> String {
    if end.is_empty() {
        format!("{when} — {kind}")
    } else {
        format!("{when} — {kind}, {end}")
    }
}
/// A running session.
pub const RUNNING: &str = "running";

// ---- report ----------------------------------------------------------------------------------

/// Report sheet title.
pub const REPORT: &str = "Create a report";
/// What the report is.
pub const REPORT_INTRO: &str = "This report helps someone else find out what went wrong. \
     Your home folder, user name and computer name are replaced. Look through the files before \
     you share them: names of games and folders can still be personal.";
/// Never included.
pub const REPORT_EXCLUDES: &str = "Never included: the contents of games' private Windows \
     storage, artwork, statistics, other games, your environment, the library file.";
/// Building.
pub const REPORT_BUILDING: &str = "Collecting…";
/// View a file.
pub const VIEW: &str = "View";
/// Hide a file.
pub const HIDE: &str = "Hide";
/// Save.
pub const SAVE_REPORT: &str = "Save report…";
/// File name proposed in the save dialog.
pub const REPORT_FILE_NAME: &str = "easyplay-report.tar.gz";
/// Total size.
pub fn report_total(files: usize, size: u64) -> String {
    format!(
        "{files} {} · {}",
        super::plural(files as u64, "file", "files"),
        bytes(size)
    )
}

// ---- Doctor ----------------------------------------------------------------------------------

/// Doctor heading in Settings → Advanced.
pub const DOCTOR: &str = "System check";
/// Doctor help.
pub const DOCTOR_HELP: &str = "Every check EasyPlay makes of this computer. Problems show \
     what to do.";
/// Doctor pending.
pub const DOCTOR_PENDING: &str = "Checking…";
/// Check again.
pub const CHECK_AGAIN: &str = "Check again";
/// Create a report.
pub const CREATE_REPORT: &str = "Create a report…";
/// Copy fix command.
pub const COPY_COMMAND: &str = "Copy command";

// ---- crash and recovery ----------------------------------------------------------------------

/// Crash banner.
pub const CRASHED: &str =
    "EasyPlay closed unexpectedly last time. Running games were not affected.";
/// Recovery banner (migration failure).
pub const READ_ONLY: &str = "Your library is read-only until the update problem is resolved. \
     Games can't be started.";
/// Damaged library heading.
pub const DAMAGED_TITLE: &str = "The library file is damaged";
/// Damaged library body.
pub const DAMAGED_BODY: &str = "EasyPlay could not repair it completely. Choose how to \
     continue; the damaged file is kept in the backups folder either way. Your games' files \
     and saves are not affected.";
/// Restore a backup.
pub fn restore_backup(date: &str, games: u64) -> String {
    format!(
        "Restore a backup ({date}, {games} {})",
        super::plural(games, "game", "games")
    )
}
/// Use repaired.
pub fn use_repaired(games: u64) -> String {
    format!(
        "Use the repaired library ({games} {} recovered)",
        super::plural(games, "game", "games")
    )
}
/// Missing games.
pub fn missing_games(names: &str) -> String {
    format!("Not recovered: {names}")
}
/// Start empty.
pub const START_EMPTY: &str = "Start with an empty library";
/// Start empty detail.
pub const START_EMPTY_NOTE: &str = "Games' private Windows storage is kept and is offered \
     again when you add a game.";
/// No backups.
pub const NO_BACKUPS: &str = "No usable backup was found.";
/// Continue.
pub const CONTINUE: &str = "Continue";
/// Quit.
pub const QUIT: &str = "Quit";
/// Open backups folder.
pub const OPEN_BACKUPS: &str = "Open backups folder";
/// Newer library heading.
pub const NEWER_TITLE: &str = "The library was created by a newer EasyPlay";
/// Advanced options note on the game page.
pub const ADVANCED_NOTE: &str = "Uses advanced options";
/// Review.
pub const REVIEW: &str = "Review";
/// Review heading.
pub const ADVANCED_HELP: &str = "These options need Developer Mode to be added. They keep \
     working while it is off.";

/// "crashed (status 139)".
pub fn end_status(reason: &str, status: i32) -> String {
    format!("{reason} (status {status})")
}
