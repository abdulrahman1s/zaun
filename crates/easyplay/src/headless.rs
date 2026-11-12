//! CLI verbs that work without a GUI. Calls the same `service::flows` as the GUI with a
//! console prompt handler. Tasks CLI-002 (read-only verbs) and CLI-003 (mutating verbs).

use easyplay_core::Error;

use crate::cli::{Command, RuntimesCommand, SandboxCommand};
use crate::{host_verbs, sandbox_print};

/// Run a verb headlessly.
pub fn run(command: &Command) -> Result<(), Error> {
    match command {
        Command::Doctor { json } => return host_verbs::doctor(*json),
        Command::Runtimes {
            command: RuntimesCommand::List,
        } => return host_verbs::runtimes_list(),
        Command::Sandbox {
            command: SandboxCommand::Print { game, format, kind },
        } => return sandbox_print::run(game, *format, *kind),
        _ => {}
    }
    let task = match command {
        Command::List { .. }
        | Command::Show { .. }
        | Command::Sandbox { .. }
        | Command::Doctor { .. }
        | Command::Runtimes { .. } => "CLI-002",
        Command::Launch(_)
        | Command::Stop { .. }
        | Command::Add { .. }
        | Command::Export { .. }
        | Command::Import { .. } => "CLI-003",
        Command::Open { .. } => "DESKTOP-002",
        Command::Supervise { .. } => "PROC-003",
    };
    Err(Error::not_available(task))
}
