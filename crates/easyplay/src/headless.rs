//! CLI verbs that work without a GUI. Calls the same `service::flows` as the GUI with a
//! console prompt handler (CLI-002 read-only verbs, CLI-003 mutating verbs). Session verbs
//! live in [`crate::session_verbs`], library verbs in [`crate::library_verbs`], host verbs in
//! [`crate::host_verbs`], `runtimes install/remove/verify` in [`crate::runtime_verbs`].

use easyplay_core::Error;

use crate::cli::{Command, LaunchArgs, RuntimesCommand, SandboxCommand, exit};
use crate::{host_verbs, library_verbs, runtime_verbs, sandbox_print, session_verbs};

/// Run a verb headlessly; returns the exit code.
pub fn run(command: &Command) -> Result<u8, Error> {
    let ok = |r: Result<(), Error>| r.map(|()| exit::OK);
    match command {
        Command::Doctor { json } => ok(host_verbs::doctor(*json)),
        Command::Runtimes {
            command: RuntimesCommand::List,
        } => ok(host_verbs::runtimes_list()),
        Command::Runtimes {
            command: RuntimesCommand::Adopt { variant },
        } => host_verbs::runtimes_adopt(*variant),
        Command::Runtimes {
            command: RuntimesCommand::Verify,
        } => runtime_verbs::verify(),
        Command::Runtimes {
            command: RuntimesCommand::Install { what },
        } => runtime_verbs::install(what),
        Command::Runtimes {
            command: RuntimesCommand::Remove { name },
        } => runtime_verbs::remove(name),
        // Automatic update checks (RUNTIME_MANAGEMENT §8); `runtimes install` checks a channel.
        Command::Runtimes {
            command: RuntimesCommand::Check,
        } => Err(Error::not_available("CLI-002")),
        Command::Sandbox {
            command: SandboxCommand::Print { game, format, kind },
        } => ok(sandbox_print::run(game, *format, *kind)),
        Command::Launch(LaunchArgs {
            game,
            exe,
            wait,
            extra,
        }) => session_verbs::launch(game, exe.as_deref(), *wait, extra),
        Command::Stop { game, force } => session_verbs::stop(game, *force),
        Command::Status { json } => session_verbs::status(*json),
        Command::Add {
            file,
            name,
            preset,
            no_launch,
            runtime,
        } => library_verbs::add(
            file,
            name.as_deref(),
            *preset,
            *no_launch,
            runtime.as_deref(),
        ),
        Command::Edit {
            game,
            runtime,
            no_follow_updates,
        } => library_verbs::edit(game, runtime.as_deref(), *no_follow_updates),
        Command::Remove { game, keep_storage } => library_verbs::remove(game, *keep_storage),
        Command::List { json, running, tag } => {
            library_verbs::list(*json, *running, tag.as_deref())
        }
        Command::Show { game, json } => library_verbs::show(game, *json),
        Command::Export { game, output } => library_verbs::export(game, output.as_deref()),
        Command::Import { file, exe } => library_verbs::import(file, exe),
        // Dispatched before reaching here.
        Command::Open { .. } | Command::Supervise { .. } | Command::GenerateAssets { .. } => {
            Err(Error::not_available("DESKTOP-002"))
        }
    }
}
