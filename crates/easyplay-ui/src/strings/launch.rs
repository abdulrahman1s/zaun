//! Launch progress and the problem panel (USER_EXPERIENCE §7, UF-9, UF-11).

use easyplay_core::model::EndReason;

/// Phase.
pub const CHECKING: &str = "Checking…";
/// Phase.
pub const DOWNLOADING: &str = "Downloading the compatibility layer (first time only)…";
/// Phase.
pub const PREPARING_STORAGE: &str = "Preparing private Windows storage…";
/// Phase.
pub const UPDATING_STORAGE: &str = "Updating private Windows storage…";
/// Phase.
pub const AUDITING: &str = "Checking the compatibility layer…";
/// Phase.
pub const STARTING: &str = "Starting…";
/// State.
pub const RUNNING: &str = "Running";
/// State.
pub const CANCELLED: &str = "Cancelled";
/// The game stays offline during the download.
pub const OFFLINE_NOTE: &str = "The game itself stays offline. Only EasyPlay downloads.";
/// Reassurance after 10 s.
pub const REASSURE: &str = "Still working. This can take a minute the first time.";
/// Button.
pub const CANCEL: &str = "Cancel";
/// Button.
pub const RUN_IN_BACKGROUND: &str = "Run in background";
/// Button.
pub const FORCE_STOP: &str = "Force stop";
/// Row.
pub const BACKGROUND: &str = "Still running in the background";
/// Row body.
pub const BACKGROUND_BODY: &str =
    "The game's window closed, but some of its programs are still running.";
/// Problem panel fallback title.
pub const CLOSED_EARLY: &str = "The game closed right after starting.";
/// Problem panel fallback body.
pub const CLOSED_EARLY_BODY: &str = "Nothing specific was found. These changes help most often.";
/// Button.
pub const VIEW_LOG: &str = "View log";
/// Button.
pub const COPY_DETAILS: &str = "Copy details";
/// Button.
pub const TRY_AGAIN: &str = "Try again";
/// Button.
pub const DISMISS: &str = "Dismiss";
/// Accessible name of the progress bar.
pub const PROGRESS: &str = "Launch progress";

/// "Ended normally", "Stopped", "Crashed", "Did not start", "Interrupted".
pub fn ended(r: EndReason) -> &'static str {
    match r {
        EndReason::Exited { .. } | EndReason::LauncherTerminated => "Ended normally",
        EndReason::StoppedByUser | EndReason::Killed | EndReason::SystemShutdown => "Stopped",
        EndReason::Crashed { .. } => "Crashed",
        EndReason::FailedToStart { .. } => "Did not start",
        EndReason::Interrupted => "Interrupted",
    }
}

/// "12.4 MB · 3.1 MB/s".
pub fn bytes_rate(bytes: u64, rate: Option<u64>) -> String {
    match rate {
        Some(r) => format!("{} · {}/s", super::fmt::bytes(bytes), super::fmt::bytes(r)),
        None => super::fmt::bytes(bytes),
    }
}
