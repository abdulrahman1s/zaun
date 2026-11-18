//! `fake-pipewire` (ADR-0026): stand-in for `pipewire -c <file>` as a session's sound server.
//!
//! It reads the socket from the configuration's `server.address`, listens there, prints what
//! it was given (the names in its environment, the length of the rule's secret, the mode of
//! the configuration) and then waits to be killed. Nothing is played or recorded.

#![allow(clippy::disallowed_methods, clippy::print_stdout, clippy::print_stderr)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::process::exit;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(config) = args
        .iter()
        .position(|a| a == "-c")
        .and_then(|i| args.get(i + 1))
    else {
        eprintln!("usage: fake-pipewire -c FILE");
        exit(2);
    };
    let text = std::fs::read_to_string(config).unwrap_or_default();
    let between = |start: &str, end: char| {
        text.split_once(start)
            .and_then(|(_, rest)| rest.split_once(end))
            .map(|(v, _)| v.to_owned())
    };
    let Some(socket) = between("\"unix:", '"') else {
        eprintln!("no unix address in {config}");
        exit(1);
    };
    let secret = between("pulse.server.type = \"!", '"').unwrap_or_default();
    let mode = std::fs::metadata(config).map_or(0, |m| m.permissions().mode() & 0o777);
    let mut names: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    names.sort();
    let _listener = UnixListener::bind(&socket).unwrap_or_else(|e| {
        eprintln!("bind {socket}: {e}");
        exit(1)
    });
    let mut out = std::io::stdout();
    let _ = writeln!(out, "listening on {socket}");
    let _ = writeln!(out, "env: {}", names.join(" "));
    let _ = writeln!(out, "secret: {} hex digits", secret.len());
    let _ = writeln!(out, "config mode: {mode:o}");
    let _ = out.flush();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
