//! `zaun`: GUI, CLI and supervisor in one binary (ARCHITECTURE §2).
//!
//! Parse the command line, install the panic hook and logging, dispatch. No other logic.

#![forbid(unsafe_code)]

mod cli;
mod dispatch;
mod display_libs;
mod game_source;
mod gen_assets;
mod headless;
mod host_verbs;
mod instance_gui;
mod library_verbs;
mod notify_watch;
mod overlay_verb;
mod panic;
mod run_gui;
mod run_service;
mod run_supervisor;
mod runtime_verbs;
mod sandbox_print;
mod session_verbs;
mod steam_verbs;
mod wiring;

use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use zaun_core::consts::{ENV_LOG, ENV_LOG_FORMAT};
use zaun_core::error::catalog::{self, DescribeCtx};
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::logging::{self, LogFormat, LogGuard, LogOptions, ProcessRole, Redactor};
use zaun_core::paths::{AppPaths, PathEnv};

use crate::cli::{Cli, Command};

fn role_of(cli: &Cli) -> ProcessRole {
    match cli.command {
        None | Some(Command::Open { .. } | Command::Service) => ProcessRole::Gui,
        Some(Command::Supervise { .. }) => ProcessRole::Supervisor,
        Some(_) => ProcessRole::Cli,
    }
}

fn redactor(env: &PathEnv, paths: Option<&AppPaths>) -> Redactor {
    let user = env
        .get("USER")
        .or_else(|| env.get("LOGNAME"))
        .and_then(|u| u.to_str())
        .map(str::to_owned);
    let host = RealFs::new()
        .read(Path::new("/proc/sys/kernel/hostname"), 256)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .map(|h| h.trim().to_owned());
    Redactor::new(
        paths.and_then(|p| p.home.as_deref()),
        user.as_deref(),
        host.as_deref(),
    )
}

/// Best effort: a CLI must work even when the state directory is not writable.
fn init_logging(cli: &Cli, env: &PathEnv) -> (Option<LogGuard>, Redactor) {
    let paths = AppPaths::resolve(env).ok();
    let redact = redactor(env, paths.as_ref());
    let mut opts = LogOptions::new(role_of(cli));
    opts.verbose = cli.verbose;
    opts.env_level = env.get(ENV_LOG).and_then(|v| v.to_str()).map(str::to_owned);
    opts.format = LogFormat::from_env_value(env.get(ENV_LOG_FORMAT).and_then(|v| v.to_str()));
    // A supervisor's stderr is its `supervisor.log` (PROCESS_LIFECYCLE §2).
    opts.console = cli.verbose > 0
        || std::io::stderr().is_terminal()
        || role_of(cli) == ProcessRole::Supervisor;
    opts.redactor = redact.clone();
    if let Some(p) = &paths
        && RealFs::new().create_dir_all(&p.logs_dir, 0o700).is_ok()
    {
        opts.file = Some(p.app_log_file.clone());
    }
    match logging::init(&opts) {
        Ok(guard) => (Some(guard), redact),
        Err(_) => {
            opts.file = None;
            (logging::init(&opts).ok(), redact)
        }
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // `--help` and `--version` print and exit 0; usage errors exit 2.
            let _ = e.print();
            return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(cli::exit::USAGE));
        }
    };
    panic::install();
    let env = PathEnv::from_process();
    let (log_guard, redact) = init_logging(&cli, &env);
    // Crash records for the GUI and the CLI; a supervisor's panic goes to its own log.
    if role_of(&cli) != ProcessRole::Supervisor
        && let Ok(p) = AppPaths::resolve(&env)
    {
        panic::configure(
            p.crash_dir.clone(),
            redact.clone(),
            role_of(&cli),
            log_guard.as_ref().map(LogGuard::recent),
        );
    }
    tracing::info!(
        version = cli::VERSION,
        role = role_of(&cli).as_str(),
        "start"
    );

    match dispatch::run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            let redact_fn = |s: &str| redact.redact_str(s);
            let ctx = DescribeCtx {
                game: None,
                session: None,
                redact: &redact_fn,
            };
            let user = catalog::describe(&err, &ctx);
            tracing::warn!(code = err.code.as_str(), "command failed: {err}");
            let mut line = format!("zaun: {}. {}", user.title, user.message);
            if let Some(task) = err.context_value("task") {
                line.push_str(&format!(" (tracked as {task})"));
            }
            // Host problems name what is missing (e.g. display libraries).
            if err.code.area() == "HOST"
                && let Some(d) = err.context_value("detail")
            {
                line.push_str(&format!(" ({})", redact.redact_str(d)));
            }
            line.push_str(&format!(" [{}]", user.code));
            #[allow(clippy::print_stderr)]
            {
                eprintln!("{line}");
            }
            ExitCode::from(dispatch::exit_code(&err))
        }
    }
}
