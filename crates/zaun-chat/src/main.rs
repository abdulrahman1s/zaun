//! `zaun-chat`: the native chat engine behind standard input and output, started by Zaun under bwrap (ADR-0042).

#![forbid(unsafe_code)]

use rustix::fs::Mode;

fn main() -> std::process::ExitCode {
    // Every file and folder the engine makes (store, received files, transfer parts) is the
    // user's alone, whatever Zaun's own umask is.
    rustix::process::umask(Mode::from_raw_mode(0o077));
    let code = zaun_chat::run(std::io::stdin(), std::io::stdout(), std::io::stderr());
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(4))
}
