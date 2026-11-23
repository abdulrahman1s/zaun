//! `zaun-chat-native`: the native chat engine behind stdin and stdout (development build, ADR-0042).

fn main() -> std::process::ExitCode {
    let code = zaun_chat::run(std::io::stdin(), std::io::stdout(), std::io::stderr());
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(4))
}
