//! Behaviour of the init shim (INIT-001). Tests outside a PID namespace rely on the shim's
//! safety guard (stop signals go to the main child only); the end-to-end tests under real
//! `bwrap` run only with `ZAUN_E2E=1`.

#![allow(
    unsafe_code,
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::io::{BufRead, BufReader, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const INIT: &str = env!("CARGO_BIN_EXE_zaun-init");
/// Descriptor number the status pipe is placed at in the child.
const STATUS_FD: i32 = 3;

/// Spawn the shim with `--status-fd 3` connected to a pipe; returns the child and the read end.
fn spawn_with_status(cmd: &[&str]) -> (Child, std::io::PipeReader) {
    let (reader, writer) = std::io::pipe().unwrap();
    let wfd = writer.as_raw_fd();
    let mut c = Command::new(INIT);
    c.args(["--status-fd", "3", "--"]).args(cmd);
    // SAFETY: dup2 is async-signal-safe; `wfd` stays open in the parent until spawn returns.
    unsafe {
        c.pre_exec(move || {
            if libc::dup2(wfd, STATUS_FD) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = c.stdin(Stdio::null()).spawn().unwrap();
    drop(writer);
    (child, reader)
}

fn run_with_status(cmd: &[&str]) -> (i32, Vec<String>) {
    let (mut child, mut reader) = spawn_with_status(cmd);
    let status = child.wait().unwrap();
    let mut text = String::new();
    reader.read_to_string(&mut text).unwrap();
    (
        status.code().unwrap_or(-1),
        text.lines().map(str::to_owned).collect(),
    )
}

fn kinds(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .map(|l| l.split(' ').next().unwrap_or(""))
        .collect()
}

#[test]
fn normal_exit_and_status_protocol() {
    let (code, lines) = run_with_status(&["/bin/sh", "-c", "exit 0"]);
    assert_eq!(code, 0);
    assert_eq!(kinds(&lines), ["started", "main-exit", "empty"]);
    assert!(lines[0]["started ".len()..].parse::<u32>().unwrap() > 0);
    assert_eq!(lines[1], "main-exit 0");
}

#[test]
fn exit_code_propagates() {
    let (code, lines) = run_with_status(&["/bin/sh", "-c", "exit 3"]);
    assert_eq!(code, 3);
    assert!(lines.contains(&"main-exit 3".to_owned()));
}

#[test]
fn signal_death_is_128_plus_n() {
    let (code, lines) = run_with_status(&["/bin/sh", "-c", "kill -TERM $$"]);
    assert_eq!(code, 143);
    assert!(lines.contains(&"main-exit 143".to_owned()));
}

#[test]
fn exec_failure_is_127() {
    let (code, lines) = run_with_status(&["/nonexistent/program"]);
    assert_eq!(code, 127);
    assert_eq!(
        kinds(&lines),
        ["started", "exec-failed", "main-exit", "empty"]
    );
    assert_eq!(lines[1], format!("exec-failed {}", libc::ENOENT));
}

#[test]
fn waits_for_detached_children() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let script = format!(
        "(sleep 1; echo done > '{}') >/dev/null 2>&1 </dev/null & exit 0",
        marker.display()
    );
    let start = Instant::now();
    let (code, lines) = run_with_status(&["/bin/sh", "-c", &script]);
    assert_eq!(code, 0);
    assert!(start.elapsed() >= Duration::from_millis(900));
    assert!(
        marker.exists(),
        "init returned before the detached child finished"
    );
    assert_eq!(kinds(&lines), ["started", "main-exit", "empty"]);
}

#[test]
fn stop_signal_is_relayed() {
    let (mut child, reader) = spawn_with_status(&[
        "/bin/sh",
        "-c",
        "trap 'kill $! 2>/dev/null; exit 7' TERM; sleep 30 & wait",
    ]);
    let mut lines = BufReader::new(reader).lines();
    assert!(lines.next().unwrap().unwrap().starts_with("started "));
    // Give the shell time to install its trap.
    std::thread::sleep(Duration::from_millis(300));
    // SAFETY: kill has no memory preconditions.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = child.wait().unwrap();
    let rest: Vec<String> = lines.map(Result::unwrap).collect();
    assert_eq!(status.code(), Some(7), "{rest:?} {:?}", status.signal());
    assert_eq!(rest[0], format!("stop-requested {}", libc::SIGTERM));
    assert!(rest.contains(&"main-exit 7".to_owned()));
    assert_eq!(rest.last().map(String::as_str), Some("empty"));
}

#[test]
fn closed_status_descriptor_is_ignored() {
    let (mut child, reader) = spawn_with_status(&["/bin/sh", "-c", "sleep 0.2; exit 5"]);
    drop(reader);
    assert_eq!(child.wait().unwrap().code(), Some(5));
}

#[test]
fn works_without_status_fd() {
    let status = Command::new(INIT)
        .args(["--", "/bin/sh", "-c", "exit 4"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(4));
}

#[test]
fn usage_errors() {
    for args in [
        &[][..],
        &["/bin/true"][..],
        &["--"][..],
        &["--status-fd"][..],
        &["--status-fd", "x", "--", "/bin/true"][..],
        &["--bogus", "--", "/bin/true"][..],
    ] {
        let out = Command::new(INIT).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(125), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"));
    }
}

// ---- end-to-end under real bwrap -------------------------------------------------------

/// Parent PID and `comm` of a host process.
fn stat_of(pid: i32) -> Option<(i32, String)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text[open + 1..close].to_owned();
    let ppid = text[close + 2..].split(' ').nth(1)?.parse().ok()?;
    Some((ppid, comm))
}

/// The `zaun-init` process below `ancestor` (bwrap → bwrap (PID 1) → shim (PID 2)).
fn find_shim(ancestor: u32) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        for entry in std::fs::read_dir("/proc").ok()?.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<i32>().ok())
            else {
                continue;
            };
            if stat_of(pid).is_some_and(|(_, comm)| comm == "zaun-init") {
                let mut cur = pid;
                for _ in 0..8 {
                    match stat_of(cur) {
                        Some((ppid, _)) if ppid == ancestor as i32 => return Some(pid),
                        Some((ppid, _)) if ppid > 1 => cur = ppid,
                        _ => break,
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

fn wait_or_kill(child: &mut Child, limit: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("sandbox did not end within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn e2e() -> bool {
    std::env::var_os("ZAUN_E2E").is_some_and(|v| v == "1")
}

fn bwrap(extra: &[&str], cmd: &[&str]) -> Command {
    let mut c = Command::new("bwrap");
    c.args([
        "--unshare-pid",
        "--unshare-user",
        "--die-with-parent",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
    ])
    .args(extra)
    // The shim is visible at its build path through the read-only root bind.
    .args(["--", INIT, "--"])
    .args(cmd);
    c
}

#[test]
fn e2e_detached_child_keeps_sandbox_alive() {
    if !e2e() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_str().unwrap();
    let start = Instant::now();
    let status = bwrap(
        &["--bind", d, d],
        &[
            "/bin/sh",
            "-c",
            &format!("(sleep 1; touch '{d}/marker') >/dev/null 2>&1 & exit 0"),
        ],
    )
    .status()
    .unwrap();
    assert!(status.success());
    assert!(start.elapsed() >= Duration::from_millis(900));
    assert!(dir.path().join("marker").exists());
}

#[test]
fn e2e_stop_reaches_setsid_grandchild() {
    if !e2e() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_str().unwrap();
    let grandchild = format!(
        "trap 'touch {d}/got-term; exit 0' TERM; touch {d}/ready; while :; do sleep 0.1; done"
    );
    let script = format!("setsid /bin/sh -c \"{grandchild}\" & while :; do sleep 0.1; done");
    let mut child = bwrap(&["--bind", d, d], &["/bin/sh", "-c", &script])
        .spawn()
        .unwrap();
    let ready = dir.path().join("ready");
    let start = Instant::now();
    while !ready.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let shim = find_shim(child.id()).expect("shim pid");
    // SAFETY: kill has no memory preconditions.
    unsafe { libc::kill(shim, libc::SIGTERM) };
    let status = wait_or_kill(&mut child, Duration::from_secs(10));
    assert!(dir.path().join("got-term").exists());
    assert!(status.code().is_some());
}
