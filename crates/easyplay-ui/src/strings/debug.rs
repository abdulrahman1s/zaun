//! Developer Mode views, the diagnostics report, the Doctor and recovery mode
//! (DEVELOPER_MODE, SCREEN_SPECS_2 §15–16, ERROR_HANDLING §4.11–4.13, §5–7).

use easyplay_core::model::EndReason;

texts! {
    /// Inspector sheet title.
    INSPECTOR = "debug.inspector";
    /// Inspector title for the dry run.
    INSPECTOR_NEXT = "debug.inspector_next";
}
/// Tab names, in order.
#[allow(non_snake_case)]
pub fn INSPECTOR_TABS() -> [&'static str; 8] {
    [
        crate::i18n::text("debug.inspector_tabs.summary"),
        crate::i18n::text("debug.inspector_tabs.access"),
        crate::i18n::text("debug.inspector_tabs.command"),
        crate::i18n::text("debug.inspector_tabs.sandbox"),
        crate::i18n::text("debug.inspector_tabs.variables"),
        crate::i18n::text("debug.inspector_tabs.processes"),
        crate::i18n::text("debug.inspector_tabs.events"),
        crate::i18n::text("debug.inspector_tabs.raw"),
    ]
}
texts! {
    /// Accessible name of the tab row.
    INSPECTOR_TAB_LABEL = "debug.inspector_tab_label";
    /// Loading.
    PLANNING = "debug.planning";
    /// Loading.
    LOADING = "debug.loading";
    /// Dry-run label.
    DRY_RUN = "debug.dry_run";
    /// Summary keys.
    K_GAME = "debug.k_game";
    /// Summary key.
    K_KIND = "debug.k_kind";
    /// Summary key.
    K_PRESET = "debug.k_preset";
    /// Summary key.
    K_NETWORK = "debug.k_network";
    /// Summary key.
    K_LAYER = "debug.k_layer";
    /// Summary key.
    K_ISOLATION = "debug.k_isolation";
    /// Summary key.
    K_PREFIX = "debug.k_prefix";
    /// Summary key.
    K_HOME = "debug.k_home";
    /// Summary key.
    K_WORKDIR = "debug.k_workdir";
    /// Summary key.
    K_SESSION = "debug.k_session";
    /// Summary key.
    K_END = "debug.k_end";
    /// Summary key.
    K_TIMELINE = "debug.k_timeline";
    /// Notes heading.
    NOTES = "debug.notes";
    /// Access tab empty.
    NO_STATEMENTS = "debug.no_statements";
    /// Command tab.
    UMU_COMMAND = "debug.umu_command";
    /// Command tab.
    FULL_COMMAND = "debug.full_command";
    /// Copy as list.
    COPY_LIST = "debug.copy_list";
    /// Copy as shell command.
    COPY_SHELL = "debug.copy_shell";
    /// What the shell copy contains.
    COPY_SHELL_NOTE = "debug.copy_shell_note";
    /// Sandbox tab.
    NAMESPACES = "debug.namespaces";
    /// Sandbox tab.
    MOUNTS = "debug.mounts";
    /// Filter field.
    FILTER = "debug.filter";
    /// Toggle.
    ONLY_WRITABLE = "debug.only_writable";
    /// Toggle.
    ONLY_CHOICES = "debug.only_choices";
}
/// Mount table headers.
#[allow(non_snake_case)]
pub fn MOUNT_COLUMNS() -> [&'static str; 7] {
    [
        "#",
        crate::i18n::text("debug.mount_columns.kind"),
        crate::i18n::text("debug.mount_columns.source"),
        crate::i18n::text("debug.mount_columns.destination"),
        crate::i18n::text("debug.mount_columns.mode"),
        crate::i18n::text("debug.mount_columns.section"),
        crate::i18n::text("debug.mount_columns.reason"),
    ]
}
texts! {
    /// Sandbox tab.
    DESCRIPTORS = "debug.descriptors";
    /// Sandbox tab.
    SECCOMP_ON = "debug.seccomp_on";
    /// Sandbox tab.
    SECCOMP_OFF = "debug.seccomp_off";
}
/// Environment headers.
#[allow(non_snake_case)]
pub fn ENV_COLUMNS() -> [&'static str; 3] {
    [
        crate::i18n::text("debug.env_columns.name"),
        crate::i18n::text("debug.env_columns.value"),
        crate::i18n::text("debug.env_columns.origin"),
    ]
}
texts! {
    /// Copy all.
    COPY_ALL = "debug.copy_all";
    /// Copy one row.
    COPY = "debug.copy";
    /// umu's own variables.
    UMU_ADDS = "debug.umu_adds";
    /// Processes tab, past session.
    PROCESSES_PAST = "debug.processes_past";
}
/// Process headers.
#[allow(non_snake_case)]
pub fn PROCESS_COLUMNS() -> [&'static str; 6] {
    [
        "PID",
        crate::i18n::text("debug.process_columns.name"),
        crate::i18n::text("debug.process_columns.role"),
        crate::i18n::text("debug.process_columns.state"),
        crate::i18n::text("debug.process_columns.cpu_time"),
        crate::i18n::text("debug.process_columns.memory"),
    ]
}
texts! {
    /// Stop session.
    STOP_SESSION = "debug.stop_session";
}
/// Events headers.
#[allow(non_snake_case)]
pub fn EVENT_COLUMNS() -> [&'static str; 3] {
    [
        crate::i18n::text("debug.event_columns.time"),
        crate::i18n::text("debug.event_columns.kind"),
        crate::i18n::text("debug.event_columns.details"),
    ]
}
texts! {
    /// No events.
    NO_EVENTS = "debug.no_events";
    /// Raw tab.
    RAW_ARGS = "debug.raw_args";
    /// Copy JSON.
    COPY_JSON = "debug.copy_json";
    /// Copy NUL-separated.
    COPY_NUL = "debug.copy_nul";
}
/// Headers of a list of arguments.
#[allow(non_snake_case)]
pub fn ARG_COLUMNS() -> [&'static str; 2] {
    ["#", crate::i18n::text("debug.arg_columns.argument")]
}
/// Headers of the descriptors table.
#[allow(non_snake_case)]
pub fn FD_COLUMNS() -> [&'static str; 3] {
    [
        "#",
        crate::i18n::text("debug.fd_columns.kind"),
        crate::i18n::text("debug.fd_columns.purpose"),
    ]
}

/// "3 processes · 1 h 2 min CPU · 1.2 GB".
pub fn process_totals(n: usize, cpu_ms: u64, rss: u64) -> String {
    crate::i18n::plural::counted_args(
        "debug.process_totals",
        n as u64,
        &[("cpu", &cpu_time(cpu_ms)), ("memory", &bytes(rss))],
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

texts! {
    /// Viewer title.
    LOGS = "debug.logs";
}
/// Viewer title with the game.
pub fn logs_of(game: &str) -> String {
    rust_i18n::t!("debug.logs_of", game = game).into_owned()
}
texts! {
    /// Session selector.
    SESSION = "debug.session";
}
/// File selector names (Developer Mode).
pub const LOG_FILES: [&str; 4] = ["game.log", "events.jsonl", "supervisor.log", "Proton log"];
/// File selector names.
#[allow(non_snake_case)]
pub fn PLAIN_LOG_FILES() -> [&'static str; 2] {
    [
        crate::i18n::text("debug.plain_log_files.game_output"),
        crate::i18n::text("debug.plain_log_files.proton_log"),
    ]
}
/// What each level shows.
#[allow(non_snake_case)]
pub fn LEVEL_HINTS() -> [&'static str; 4] {
    [
        crate::i18n::text("debug.level_hints.only_errors"),
        crate::i18n::text("debug.level_hints.errors_and_warnings"),
        crate::i18n::text("debug.level_hints.everything_but_debugging_detail"),
        crate::i18n::text("debug.level_hints.everything_with_wine_s"),
    ]
}
texts! {
    /// Program selector.
    SOURCE = "debug.source";
}
/// Programs; `All` first.
#[allow(non_snake_case)]
pub fn SOURCES() -> [&'static str; 5] {
    [
        crate::i18n::text("debug.sources.all_programs"),
        "Wine",
        "Proton",
        "umu",
        crate::i18n::text("debug.sources.game"),
    ]
}
texts! {
    /// No Proton log was recorded for the session.
    NO_PROTON_LOG = "debug.no_proton_log";
}
/// The usual name of a signal a game died from.
pub fn signal_name(signal: i32) -> Option<&'static str> {
    Some(match signal {
        4 => crate::i18n::text("debug.signal_name.illegal_instruction"),
        6 => crate::i18n::text("debug.signal_name.it_aborted_itself"),
        7 => crate::i18n::text("debug.signal_name.bus_error"),
        8 => crate::i18n::text("debug.signal_name.arithmetic_error"),
        9 => crate::i18n::text("debug.signal_name.it_was_killed"),
        11 => crate::i18n::text("debug.signal_name.segmentation_fault_it_read"),
        15 => crate::i18n::text("debug.signal_name.it_was_asked_to"),
        _ => return None,
    })
}
/// How a session ended, in a sentence, and whether that is bad.
pub fn how_it_ended(reason: Option<EndReason>, running: bool) -> (String, bool) {
    let say = |key: &str| crate::i18n::text(key).to_owned();
    let Some(r) = reason else {
        return if running {
            (say("debug.ended.running_now"), false)
        } else {
            (say("debug.ended.not_recorded"), false)
        };
    };
    match r {
        EndReason::Exited { code: 0 } => (say("debug.ended.exited_normally"), false),
        EndReason::Exited { code } => (
            rust_i18n::t!("debug.ended.exited_with_code", code = code).into_owned(),
            true,
        ),
        EndReason::Crashed { signal } => (
            match signal_name(signal) {
                Some(n) => rust_i18n::t!("debug.ended.crashed_named", signal = signal, name = n)
                    .into_owned(),
                None => rust_i18n::t!("debug.ended.crashed", signal = signal).into_owned(),
            },
            true,
        ),
        EndReason::StoppedByUser => (say("debug.ended.stopped_by_user"), false),
        EndReason::Killed => (say("debug.ended.killed"), true),
        EndReason::SystemShutdown => (say("debug.ended.shutdown"), false),
        EndReason::LauncherTerminated => (say("debug.ended.launcher_closed"), false),
        EndReason::Interrupted => (say("debug.ended.interrupted"), true),
        EndReason::FailedToStart { code } => (
            rust_i18n::t!("debug.ended.failed_to_start", code = code.as_str()).into_owned(),
            true,
        ),
    }
}
/// "Started today 21:04 · ran 5 min 6 s".
pub fn ran(started: &str, duration: Option<&str>) -> String {
    match duration {
        Some(d) => rust_i18n::t!("debug.ran", started = started, duration = d).into_owned(),
        None => rust_i18n::t!("debug.started", started = started).into_owned(),
    }
}
texts! {
    /// File selector label.
    LOG_FILE = "debug.log_file";
    /// Toggle.
    FOLLOW = "debug.follow";
    /// Open session folder.
    OPEN_FOLDER = "debug.open_folder";
    /// Open inspector.
    OPEN_INSPECTOR = "debug.open_inspector";
    /// Removed.
    LOG_REMOVED = "debug.log_removed";
}
/// Older lines dropped.
pub fn dropped(n: usize) -> String {
    rust_i18n::t!("debug.dropped", n = n).into_owned()
}
/// Session option label.
pub fn session_label(when: &str, kind: &str, end: &str) -> String {
    if end.is_empty() {
        rust_i18n::t!("debug.session_label", when = when, kind = kind).into_owned()
    } else {
        rust_i18n::t!(
            "debug.session_label_ended",
            when = when,
            kind = kind,
            end = end
        )
        .into_owned()
    }
}
texts! {
    /// A running session.
    RUNNING = "debug.running";
}

// ---- report ----------------------------------------------------------------------------------

texts! {
    /// Report sheet title.
    REPORT = "debug.report";
    /// Never included.
    REPORT_EXCLUDES = "debug.report_excludes";
    /// Building.
    REPORT_BUILDING = "debug.report_building";
    /// View a file.
    VIEW = "debug.view";
    /// Hide a file.
    HIDE = "debug.hide";
    /// Save.
    SAVE_REPORT = "debug.save_report";
}
/// File name proposed in the save dialog.
pub const REPORT_FILE_NAME: &str = "easyplay-report.tar.gz";

// ---- Doctor ----------------------------------------------------------------------------------

texts! {
    /// Doctor heading in Settings → Advanced.
    DOCTOR = "debug.doctor";
    /// Doctor pending.
    DOCTOR_PENDING = "debug.doctor_pending";
    /// Check again.
    CHECK_AGAIN = "debug.check_again";
    /// Create a report.
    CREATE_REPORT = "debug.create_report";
    /// Copy fix command.
    COPY_COMMAND = "debug.copy_command";
    /// Help before a command when the check gives no explanation of its own.
    TO_FIX = "debug.to_fix";
    /// The runtime check's fix: adopt the runtime umu already installed (D-15).
    USE_UMU_RUNTIME = "debug.use_umu_runtime";
    /// While umu's runtime is copied.
    ADOPTING = "debug.adopting";
}

// ---- crash and recovery ----------------------------------------------------------------------

texts! {
    /// Crash banner.
    CRASHED = "debug.crashed";
    /// Recovery banner (migration failure).
    READ_ONLY = "debug.read_only";
    /// Damaged library heading.
    DAMAGED_TITLE = "debug.damaged_title";
    /// Damaged library body.
    DAMAGED_BODY = "debug.damaged_body";
    /// Choice.
    RESTORE_BACKUP = "debug.restore_backup";
}
/// The backup under "Restore a backup": "Yesterday, 12 games".
pub fn backup_note(date: &str, games: u64) -> String {
    crate::i18n::plural::counted_args("debug.backup_note", games, &[("date", date)])
}
texts! {
    /// Choice.
    USE_REPAIRED = "debug.use_repaired";
}
/// What repair saved: "9 games recovered, 2 could not be read".
pub fn repaired_note(recovered: u64, unread: usize) -> String {
    let saved = crate::i18n::plural::counted("debug.recovered_games", recovered);
    if unread == 0 {
        saved
    } else {
        rust_i18n::t!("debug.repaired_unread", saved = saved, unread = unread).into_owned()
    }
}
/// The games repair could not read, named in the tooltip of "Use the repaired library".
pub fn missing_games(names: &str) -> String {
    rust_i18n::t!("debug.missing_games", names = names).into_owned()
}
/// A recovery choice read whole: "Use the repaired library. 9 games recovered, 2 could not
/// be read. Not recovered: Celeste, Hades".
pub fn choice_spoken(parts: &[&str]) -> String {
    parts.join(". ")
}
texts! {
    /// Start empty.
    START_EMPTY = "debug.start_empty";
    /// Start empty detail.
    START_EMPTY_NOTE = "debug.start_empty_note";
    /// No backups.
    NO_BACKUPS = "debug.no_backups";
    /// Continue.
    CONTINUE = "debug.continue";
    /// Quit.
    QUIT = "debug.quit";
    /// Open backups folder.
    OPEN_BACKUPS = "debug.open_backups";
    /// Advanced options note on the game page.
    ADVANCED_NOTE = "debug.advanced_note";
    /// Review.
    REVIEW = "debug.review";
    /// Review heading.
    ADVANCED_HELP = "debug.advanced_help";
}

/// "crashed (status 139)".
pub fn end_status(reason: &str, status: i32) -> String {
    rust_i18n::t!("debug.end_status", reason = reason, status = status).into_owned()
}
