//! `fake-umu-run` (PROC-013, MODULES §11): scripted stand-in for `umu-run`.
//!
//! `fake-umu-run [EXE [ARG…]]` runs the steps of `FAKE_UMU` (comma-separated, in order):
//!
//! | Step | Effect |
//! |---|---|
//! | `umu` | print umu's version and `Running '…' using runtime 'sniper'` lines (phases) |
//! | `game` | set this process's `comm` to EXE's file name, like a Wine game process |
//! | `print:TEXT` / `stderr:TEXT` | one line on stdout / stderr |
//! | `traceback` | a Python traceback ending in `RuntimeError` (a umu failure) |
//! | `sleep:MS` | sleep |
//! | `detach:MS` | start a double-forked, `setsid` process that lives MS ms, return at once |
//! | `children:N:MS` | N children living MS ms, every second one detached |
//! | `ignore-term` | ignore `SIGTERM` from now on |
//! | `segv` / `abort` | die by `SIGSEGV` / `SIGABRT` |
//! | `spam:MIB` | write MIB MiB of output lines |
//! | `exit:N` | exit with N (default at the end: 0) |
//!
//! `fake-umu-run --version` prints `umu-launcher version 1.4.4 (fake)`.

#![allow(clippy::disallowed_methods, clippy::print_stdout, clippy::print_stderr)]

use std::ffi::CString;
use std::io::Write;
use std::process::{Command, Stdio, exit};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Variable selecting the behaviour.
const ENV_SPEC: &str = "FAKE_UMU";
/// Internal: role of a helper process (`detach1:MS`, `detach2:MS`, `child:MS`).
const ENV_ROLE: &str = "FAKE_UMU_ROLE";

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn set_comm(name: &str) {
    let mut n = name.as_bytes().to_vec();
    n.truncate(15);
    if let Ok(c) = CString::new(n) {
        let _ = rustix::thread::set_name(&c);
    }
}

fn helper(role: &str, ms: u64) {
    let exe = std::env::current_exe().unwrap_or_else(|_| "fake-umu-run".into());
    let _ = Command::new(exe)
        .env(ENV_ROLE, format!("{role}:{ms}"))
        .env_remove(ENV_SPEC)
        .stdin(Stdio::null())
        .spawn()
        .map(|mut c| {
            if role == "detach1" {
                // The intermediate exits at once; reap it so the grandchild is orphaned.
                let _ = c.wait();
            }
        });
}

fn run_role(role: &str) -> ! {
    let (kind, ms) = role.split_once(':').unwrap_or((role, "0"));
    let ms: u64 = ms.parse().unwrap_or(0);
    match kind {
        "detach1" => {
            helper("detach2", ms);
            exit(0);
        }
        "detach2" => {
            let _ = rustix::process::setsid();
            set_comm("fake-detached");
            sleep_ms(ms);
            exit(0);
        }
        _ => {
            set_comm("fake-child");
            sleep_ms(ms);
            exit(0);
        }
    }
}

fn main() {
    if let Ok(role) = std::env::var(ENV_ROLE) {
        run_role(&role);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--version") {
        println!("umu-launcher version 1.4.4 (fake)");
        return;
    }
    let exe_name = args
        .first()
        .and_then(|a| std::path::Path::new(a).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game.exe".into());
    let spec = std::env::var(ENV_SPEC).unwrap_or_default();
    let mut code = 0;
    let mut out = std::io::stdout();
    for step in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, arg) = step.split_once(':').unwrap_or((step, ""));
        match name {
            "umu" => {
                println!("INFO: umu-launcher version 1.4.4 (fake)");
                println!("INFO: Running 'UMU-Proton-10.0-4' using runtime 'sniper'");
            }
            "game" => set_comm(&exe_name),
            "print" => println!("{arg}"),
            "stderr" => eprintln!("{arg}"),
            "traceback" => {
                eprintln!("Traceback (most recent call last):");
                eprintln!("  File \"/usr/bin/umu-run\", line 8, in <module>");
                eprintln!("    sys.exit(main())");
                eprintln!("RuntimeError: fake failure");
            }
            "sleep" => sleep_ms(arg.parse().unwrap_or(0)),
            "detach" => helper("detach1", arg.parse().unwrap_or(0)),
            "children" => {
                let (n, ms) = arg.split_once(':').unwrap_or((arg, "1000"));
                let ms: u64 = ms.parse().unwrap_or(1000);
                for i in 0..n.parse::<u32>().unwrap_or(0) {
                    helper(if i % 2 == 0 { "child" } else { "detach1" }, ms);
                }
            }
            "ignore-term" => {
                let _ = signal_hook::flag::register(
                    signal_hook::consts::SIGTERM,
                    Arc::new(AtomicBool::new(false)),
                );
            }
            "segv" => {
                let _ = std::io::stdout().flush();
                // The standard library's guard-page handler swallows the first SIGSEGV that
                // is not a fault and restores the default action; the second one kills.
                for _ in 0..5 {
                    let _ = rustix::process::kill_process(
                        rustix::process::getpid(),
                        rustix::process::Signal::SEGV,
                    );
                    sleep_ms(50);
                }
            }
            "abort" => std::process::abort(),
            "spam" => {
                let mib: usize = arg.parse().unwrap_or(1);
                let line = format!("{}\n", "spam ".repeat(20));
                let mut chunk = String::new();
                while chunk.len() < 64 * 1024 {
                    chunk.push_str(&line);
                }
                let mut sent = 0;
                while sent < mib << 20 {
                    if out.write_all(chunk.as_bytes()).is_err() {
                        break;
                    }
                    sent += chunk.len();
                }
            }
            "exit" => {
                code = arg.parse().unwrap_or(0);
                break;
            }
            other => {
                eprintln!("fake-umu-run: unknown step {other:?}");
                code = 2;
                break;
            }
        }
        let _ = out.flush();
    }
    let _ = out.flush();
    exit(code);
}
