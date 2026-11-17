//! Decide: forward to a running instance, run headless, or become the GUI
//! (DESKTOP_INTEGRATION §5.1).
//!
//! ```text
//! supervise, generate-assets           → run, never touch the instance socket
//! service                               → the resident service (waits while a GUI runs)
//! read-only verbs (list, show, status, sandbox print, doctor, runtimes, export)
//!                                       → headless against the database
//! GUI running (socket answers ping in 300 ms)
//!   open / launch / stop / add / (no verb, --add) → forward, print the result
//!   import, remove                       → headless (answers "busy" while the GUI holds the library)
//! no GUI
//!   (no verb), open, --add               → become the GUI
//!   launch, stop, add, import, remove    → headless
//! ```

use std::io::IsTerminal;
use std::path::PathBuf;

use easyplay_core::desktop::instance::{
    self, InstanceTransport, RaiseThen, Request, SocketTransport, StreamEvent, WireOrigin,
};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::{Error, ErrorCode, err};

use crate::cli::{Cli, Command, LaunchArgs, exit};
use crate::{gen_assets, headless, library_verbs, run_gui, run_service, run_supervisor};

/// Where a command line goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// `supervise`.
    Supervise,
    /// `generate-assets`.
    Generate,
    /// `service`: the resident service.
    Service,
    /// Run in this process without a window.
    Headless,
    /// Start the window (no GUI is running).
    Gui {
        /// Programs to open.
        open: Vec<PathBuf>,
        /// Open the add-game picker.
        add: bool,
    },
    /// Hand over to the running GUI.
    Forward,
}

/// Whether the verb only reads (runs headless even while the GUI runs).
fn read_only(c: &Command) -> bool {
    matches!(
        c,
        Command::List { .. }
            | Command::Show { .. }
            | Command::Status { .. }
            | Command::Sandbox { .. }
            | Command::Doctor { .. }
            | Command::Runtimes { .. }
            | Command::Export { .. }
    )
}

/// The decision table of §5.1.
pub fn decide(cli: &Cli, gui_running: bool) -> Route {
    match &cli.command {
        Some(Command::Supervise { .. }) => Route::Supervise,
        Some(Command::GenerateAssets { .. }) => Route::Generate,
        Some(Command::Service) => Route::Service,
        Some(c) if read_only(c) => Route::Headless,
        // Steam requests go to Steam, not through the running window (ADR-0031).
        Some(Command::Steam { .. }) => Route::Headless,
        Some(Command::Import { .. } | Command::Remove { .. }) => Route::Headless,
        _ if gui_running => Route::Forward,
        None => Route::Gui {
            open: Vec::new(),
            add: cli.add,
        },
        Some(Command::Open { files }) => Route::Gui {
            open: files.clone(),
            add: cli.add,
        },
        Some(_) => Route::Headless,
    }
}

/// Whether the verb is one that may need the running GUI.
fn may_forward(cli: &Cli) -> bool {
    matches!(decide(cli, true), Route::Forward)
}

/// Run the parsed command line; returns the exit code of a successful run.
pub fn run(cli: Cli) -> Result<u8, Error> {
    let env = PathEnv::from_process();
    let transport = if may_forward(&cli) {
        AppPaths::resolve(&env)
            .ok()
            .map(|p| SocketTransport::new(p.instance_socket))
            .filter(InstanceTransport::ping)
    } else {
        None
    };
    match decide(&cli, transport.is_some()) {
        Route::Supervise => match &cli.command {
            Some(Command::Supervise { plan }) => run_supervisor::run(plan),
            _ => Ok(exit::USAGE),
        },
        Route::Generate => match &cli.command {
            Some(Command::GenerateAssets { out }) => gen_assets::run(out),
            _ => Ok(exit::USAGE),
        },
        Route::Service => run_service::run(),
        Route::Headless => match &cli.command {
            Some(c) => headless::run(c),
            None => Ok(exit::USAGE),
        },
        Route::Gui { open, add } => run_gui::run(open, add, cli.preview),
        Route::Forward => match transport {
            Some(t) => forward(&cli, &env, &t),
            None => Err(Error::new(ErrorCode::DskInstanceUnresponsive)),
        },
    }
}

/// The request a command line becomes when a GUI is running.
pub fn request_for(cli: &Cli, env: &PathEnv) -> Result<Request, Error> {
    let token = instance::activation_token(env);
    let cwd = std::env::current_dir().ok();
    Ok(match &cli.command {
        None => Request::Raise {
            activation_token: token,
            then: cli.add.then_some(RaiseThen::AddGame),
        },
        Some(Command::Open { files }) if files.is_empty() => Request::Raise {
            activation_token: token,
            then: cli.add.then_some(RaiseThen::AddGame),
        },
        Some(Command::Open { files }) => Request::Open {
            paths: files.clone(),
            // A terminal types `easyplay open`; a file manager starts it without one.
            origin: if std::io::stdin().is_terminal() {
                WireOrigin::Cli
            } else {
                WireOrigin::FileManager
            },
            cwd,
            activation_token: token,
        },
        Some(Command::Add { file, .. }) => Request::Open {
            paths: vec![file.clone()],
            origin: WireOrigin::Add,
            cwd,
            activation_token: token,
        },
        Some(Command::Launch(LaunchArgs {
            game,
            exe,
            wait,
            extra,
        })) => {
            // Resolve here (read-only), so names, prefixes and program labels work and
            // mistakes are reported by this process.
            let (snap, mut db, _) = library_verbs::read_snapshot()?;
            let id = library_verbs::resolve(&snap, game)?;
            // Steam starts its games: nothing to wait for, no program to choose (ADR-0031).
            if id.is_steam() && (*wait || exe.is_some() || !extra.is_empty()) {
                return Err(Error::new(ErrorCode::StmNotForSteam).with(
                    "detail",
                    "--exe, --wait or extra arguments are not possible for a game Steam runs",
                ));
            }
            let exe = match exe {
                Some(a) => {
                    use easyplay_core::storage::GameRepo;
                    let g = db
                        .game(id)?
                        .ok_or_else(|| err!(LibGameNotFound, game = id))?;
                    Some(library_verbs::resolve_exe(&g, a)?.to_string())
                }
                None => None,
            };
            Request::Launch {
                game: id.to_string(),
                exe,
                extra_args: extra.clone(),
                wait: *wait,
            }
        }
        Some(Command::Stop { game, force }) => Request::Stop {
            game: game.clone(),
            force: *force,
        },
        Some(_) => return Err(Error::not_available("DESKTOP-002")),
    })
}

/// Send the request to the running GUI and print the outcome.
#[allow(clippy::print_stdout, clippy::print_stderr)]
fn forward(cli: &Cli, env: &PathEnv, t: &dyn InstanceTransport) -> Result<u8, Error> {
    if let Some(Command::Add {
        name,
        preset,
        no_launch,
        ..
    }) = &cli.command
        && (name.is_some() || preset.is_some() || *no_launch)
    {
        eprintln!(
            "easyplay: EasyPlay is open; --name, --preset and --no-launch are chosen in its \
             setup window instead"
        );
    }
    let req = request_for(cli, env)?;
    let mut code = exit::OK;
    let resp = t.send(&req, &mut |ev| match ev {
        StreamEvent::State { state } => println!("{state}"),
        StreamEvent::Ended { reason, exit_code } => {
            println!(
                "ended: {reason}{}",
                exit_code
                    .map(|c| format!(", exit status {c}"))
                    .unwrap_or_default()
            );
            code = match reason.as_str() {
                "failed_to_start" => 1,
                "crashed" => 2,
                _ => exit::OK,
            };
        }
    })?;
    if !resp.ok {
        let c = resp.code.unwrap_or_default();
        eprintln!(
            "easyplay: {} [{c}]",
            resp.message.unwrap_or_else(|| "refused".into())
        );
        let busy = [ErrorCode::SesAlreadyRunning, ErrorCode::LibBusy];
        return Ok(if busy.iter().any(|b| b.as_str() == c) {
            exit::BUSY
        } else {
            exit::FAILED
        });
    }
    if matches!(
        req,
        Request::Open { .. } | Request::Launch { wait: false, .. }
    ) {
        println!("handed over to the running EasyPlay");
    }
    Ok(code)
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
    use clap::Parser;

    fn cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("easyplay").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn exit_codes() {
        assert_eq!(exit_code(&Error::new(ErrorCode::HostBwrapNotFound)), 4);
        assert_eq!(exit_code(&Error::new(ErrorCode::SesAlreadyRunning)), 3);
        assert_eq!(exit_code(&Error::new(ErrorCode::DbSessionsRunning)), 5);
        assert_eq!(exit_code(&Error::not_available("CLI-002")), 1);
    }

    /// Every verb with and without a running GUI (DESKTOP_INTEGRATION §11).
    #[test]
    fn dispatch_table() {
        let gui = |add: bool, open: &[&str]| Route::Gui {
            open: open.iter().map(PathBuf::from).collect(),
            add,
        };
        let table: &[(&[&str], Route, Route)] = &[
            (&[], gui(false, &[]), Route::Forward),
            (&["--add"], gui(true, &[]), Route::Forward),
            (&["open"], gui(false, &[]), Route::Forward),
            (&["open", "a.exe"], gui(false, &["a.exe"]), Route::Forward),
            (&["launch", "g"], Route::Headless, Route::Forward),
            (&["launch", "g", "--wait"], Route::Headless, Route::Forward),
            (&["stop", "g"], Route::Headless, Route::Forward),
            (&["add", "a.exe"], Route::Headless, Route::Forward),
            (
                &["import", "f.toml", "--exe", "a.exe"],
                Route::Headless,
                Route::Headless,
            ),
            (&["remove", "g"], Route::Headless, Route::Headless),
            (&["export", "g"], Route::Headless, Route::Headless),
            (&["status"], Route::Headless, Route::Headless),
            (&["list"], Route::Headless, Route::Headless),
            (&["show", "g"], Route::Headless, Route::Headless),
            (&["sandbox", "print", "g"], Route::Headless, Route::Headless),
            (&["doctor"], Route::Headless, Route::Headless),
            (&["runtimes", "list"], Route::Headless, Route::Headless),
            (
                &["supervise", "--plan", "/p"],
                Route::Supervise,
                Route::Supervise,
            ),
            (&["service"], Route::Service, Route::Service),
            (
                &["generate-assets", "--out", "/o"],
                Route::Generate,
                Route::Generate,
            ),
        ];
        for (args, without, with) in table {
            let c = cli(args);
            assert_eq!(&decide(&c, false), without, "{args:?} without a GUI");
            assert_eq!(&decide(&c, true), with, "{args:?} with a GUI");
        }
    }

    #[test]
    fn requests_carry_token_and_cwd() {
        let env = PathEnv::new(1000).with("XDG_ACTIVATION_TOKEN", "tok");
        match request_for(&cli(&["--add"]), &env).unwrap() {
            Request::Raise {
                activation_token,
                then,
            } => {
                assert_eq!(activation_token.as_deref(), Some("tok"));
                assert_eq!(then, Some(RaiseThen::AddGame));
            }
            r => panic!("{r:?}"),
        }
        match request_for(&cli(&["open", "x.exe"]), &env).unwrap() {
            Request::Open { paths, cwd, .. } => {
                assert_eq!(paths, vec![PathBuf::from("x.exe")]);
                assert!(cwd.is_some());
            }
            r => panic!("{r:?}"),
        }
        assert!(matches!(
            request_for(&cli(&["stop", "g", "--force"]), &env).unwrap(),
            Request::Stop { force: true, .. }
        ));
    }
}
