//! Read the plan file, construct supervisor dependencies, call `session::supervisor::run`.
//! Must not initialise eframe, fonts, GPU or the instance socket. Task PROC-003.

use std::path::Path;

use easyplay_core::Error;

/// Supervise the session described by `plan`.
pub fn run(_plan: &Path) -> Result<(), Error> {
    Err(Error::not_available("PROC-003"))
}
