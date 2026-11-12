//! Construct the real implementations of every trait, start the AppService and call
//! `easyplay_ui::run`. Never reachable from the supervisor path. Task UI-001.

use easyplay_core::Error;

/// Start the GUI (optionally with files to open in the quick-launch layout).
pub fn run(_open: Vec<std::path::PathBuf>) -> Result<(), Error> {
    Err(Error::not_available("UI-001"))
}
