//! The development helper end to end: Hello and Refresh in, Features, Ready and Chats out, exit 0 on EOF.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use zaun_chat_wire::{EvLine, Event, Hello, ReqLine, Request, decode_line, encode_line};

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the test starts the helper it tests"
)]
fn the_helper_starts_answers_and_exits_zero_when_stdin_closes() {
    let dir = std::env::temp_dir().join(format!("zaun-engine-pipe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_zaun-chat"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let hello = Hello::new(dir.clone(), "key".into(), "alice".into(), Vec::new());
    writeln!(stdin, "{}", encode_line(&hello).unwrap()).unwrap();
    writeln!(
        stdin,
        "{}",
        encode_line(&ReqLine {
            req: Request::Refresh
        })
        .unwrap()
    )
    .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next = || {
        decode_line::<EvLine>(&lines.next().unwrap().unwrap())
            .unwrap()
            .ev
    };
    assert!(matches!(next(), Event::Features { .. }));
    let Event::Ready(p) = next() else { panic!() };
    assert_eq!(p.name, "alice");
    assert!(matches!(next(), Event::Chats { .. }));
    assert!(matches!(next(), Event::Chats { .. }), "the refresh");
    drop(stdin);
    let started = Instant::now();
    let status = child.wait().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}
