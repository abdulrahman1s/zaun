//! Decide: forward to a running instance, run headless, or become the GUI
//! (DESKTOP_INTEGRATION §5). Instance forwarding arrives with DESKTOP-002; until then every
//! verb runs headless and the GUI path is taken directly.

use easyplay_core::{Error, ErrorCode};

use crate::cli::{Cli, Command, exit};
use crate::{headless, run_gui, run_supervisor};

/// Run the parsed command line.
pub fn run(cli: Cli) -> Result<(), Error> {
    match cli.command {
        None => run_gui::run(Vec::new()),
        Some(Command::Supervise { plan }) => run_supervisor::run(&plan),
        Some(Command::Open { files }) if files.is_empty() => run_gui::run(Vec::new()),
        Some(command) => headless::run(&command),
    }
}

/// The process exit code for an error (CONFIGURATION §6).
pub fn exit_code(err: &Error) -> u8 {
    match err.code {
        ErrorCode::SesAlreadyRunning | ErrorCode::LibBusy | ErrorCode::DbBusy => exit::BUSY,
        ErrorCode::DbSessionsRunning => exit::MIGRATION,
        c if c.area() == "HOST" => exit::HOST,
        _ => exit::FAILED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(exit_code(&Error::new(ErrorCode::HostBwrapNotFound)), 4);
        assert_eq!(exit_code(&Error::new(ErrorCode::SesAlreadyRunning)), 3);
        assert_eq!(exit_code(&Error::new(ErrorCode::DbSessionsRunning)), 5);
        assert_eq!(exit_code(&Error::not_available("CLI-002")), 1);
    }
}
