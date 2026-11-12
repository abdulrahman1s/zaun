//! CLI verbs that work without a GUI. Calls the same `service::flows` as the GUI with a
//! console prompt handler. Tasks CLI-002 (read-only verbs) and CLI-003 (mutating verbs).
//! M6 provides `launch` (without `--exe`), `stop` and `status` ([`crate::session_verbs`]).

use easyplay_core::Error;

use crate::cli::{Command, LaunchArgs, RuntimesCommand, SandboxCommand, exit};
use crate::{host_verbs, sandbox_print, session_verbs};

/// Run a verb headlessly; returns the exit code.
pub fn run(command: &Command) -> Result<u8, Error> {
    let ok = |r: Result<(), Error>| r.map(|()| exit::OK);
    match command {
        Command::Doctor { json } => return ok(host_verbs::doctor(*json)),
        Command::Runtimes {
            command: RuntimesCommand::List,
        } => return ok(host_verbs::runtimes_list()),
        Command::Sandbox {
            command: SandboxCommand::Print { game, format, kind },
        } => return ok(sandbox_print::run(game, *format, *kind)),
        Command::Launch(LaunchArgs {
            game,
            exe: None,
            wait,
            extra,
        }) => return session_verbs::launch(game, *wait, extra),
        Command::Stop { game, force } => return session_verbs::stop(game, *force),
        Command::Status { json } => return session_verbs::status(*json),
        _ => {}
    }
    let task = match command {
        Command::List { .. }
        | Command::Show { .. }
        | Command::Sandbox { .. }
        | Command::Doctor { .. }
        | Command::Runtimes { .. } => "CLI-002",
        Command::Status { .. } => "PROC-011",
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
