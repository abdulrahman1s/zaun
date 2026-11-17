//! Launch progress and the problem panel (USER_EXPERIENCE §7, UF-9, UF-11).

use easyplay_core::model::EndReason;

texts! {
    /// Phase.
    CHECKING = "launch.checking";
    /// Phase.
    DOWNLOADING = "launch.downloading";
    /// Phase.
    PREPARING_STORAGE = "launch.preparing_storage";
    /// Phase.
    UPDATING_STORAGE = "launch.updating_storage";
    /// Phase.
    AUDITING = "launch.auditing";
    /// Phase.
    STARTING = "launch.starting";
    /// State.
    RUNNING = "launch.running";
    /// State.
    CANCELLED = "launch.cancelled";
    /// The game stays offline during the download.
    OFFLINE_NOTE = "launch.offline_note";
    /// Reassurance after 10 s.
    REASSURE = "launch.reassure";
    /// Button.
    CANCEL = "launch.cancel";
    /// Button.
    FORCE_STOP = "launch.force_stop";
    /// Problem panel fallback title.
    CLOSED_EARLY = "launch.closed_early";
    /// Problem panel fallback body.
    CLOSED_EARLY_BODY = "launch.closed_early_body";
    /// Button.
    VIEW_LOG = "launch.view_log";
    /// Button.
    COPY_DETAILS = "launch.copy_details";
    /// Button.
    TRY_AGAIN = "launch.try_again";
    /// Button.
    DISMISS = "launch.dismiss";
    /// Accessible name of the progress bar.
    PROGRESS = "launch.progress";
}

/// "Ended normally", "Stopped", "Crashed", "Did not start", "Interrupted".
pub fn ended(r: EndReason) -> &'static str {
    match r {
        EndReason::Exited { .. } | EndReason::LauncherTerminated => {
            crate::i18n::text("launch.ended.ended_normally")
        }
        EndReason::StoppedByUser | EndReason::Killed | EndReason::SystemShutdown => {
            crate::i18n::text("launch.ended.stopped")
        }
        EndReason::Crashed { .. } => crate::i18n::text("launch.ended.crashed"),
        EndReason::FailedToStart { .. } => crate::i18n::text("launch.ended.did_not_start"),
        EndReason::Interrupted => crate::i18n::text("launch.ended.interrupted"),
    }
}

/// "12.4 MB · 3.1 MB/s".
pub fn bytes_rate(bytes: u64, rate: Option<u64>) -> String {
    match rate {
        Some(r) => rust_i18n::t!(
            "launch.bytes_rate",
            bytes = super::fmt::bytes(bytes),
            rate = super::fmt::bytes(r)
        )
        .into_owned(),
        None => super::fmt::bytes(bytes),
    }
}

/// Quick launch when the game is already running.
pub fn already_running(name: &str) -> String {
    rust_i18n::t!("launch.already_running", name = name).into_owned()
}
