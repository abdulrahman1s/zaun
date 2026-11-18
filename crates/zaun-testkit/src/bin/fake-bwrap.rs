//! `fake-bwrap` (PROC-013, TESTING_STRATEGY §6): stands in for bubblewrap in supervisor tests
//! on machines without user namespaces.
//!
//! Parses the options Zaun emits (including those read from `--args FD`), checks the
//! ordering rules (`--perms`/`--size` immediately before an option that takes them, every
//! option known and complete), applies `--setenv`/`--unsetenv`/`--clearenv`/`--chdir`, maps
//! the command's program through the bind mounts (so `/run/zaun/init` becomes the host
//! path of the init shim), runs it **without namespaces** with `PR_SET_PDEATHSIG(SIGKILL)`
//! for `--die-with-parent`, writes `{"child-pid": …, "pid-namespace": …}` and
//! `{"exit-code": …}` to `--json-status-fd`, holds `--sync-fd` like the real sandbox, and
//! exits with the command's status.
//!
//! The reported `pid-namespace` is the caller's own, which tells the supervisor that the
//! sandbox has no namespace of its own (it then tracks descendants instead).
//!
//! `fake-bwrap --version` prints `bubblewrap 0.12.0`.

#![allow(unsafe_code, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use zaun_core::launch::plan_file::option_arity;

/// Options without a value that bubblewrap 0.8–0.13 knows.
const FLAGS: &[&str] = &[
    "--unshare-all",
    "--share-net",
    "--unshare-user",
    "--unshare-user-try",
    "--unshare-ipc",
    "--unshare-pid",
    "--unshare-net",
    "--unshare-uts",
    "--unshare-cgroup",
    "--unshare-cgroup-try",
    "--disable-userns",
    "--assert-userns-disabled",
    "--clearenv",
    "--new-session",
    "--die-with-parent",
    "--as-pid-1",
    "--level-prefix",
];

/// Options `--perms` may precede.
const PERMS_TARGETS: &[&str] = &[
    "--tmpfs",
    "--dir",
    "--file",
    "--bind-data",
    "--ro-bind-data",
    "--chmod",
];

fn fail(msg: &str) -> ! {
    eprintln!("bwrap: {msg}");
    exit(1);
}

fn read_args_fd(fd: i32) -> Vec<OsString> {
    // SAFETY: the descriptor was handed to us for exactly this purpose; it is read once.
    let mut f = unsafe { File::from_raw_fd(fd) };
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        fail(&format!("Failed to read --args {fd}"));
    }
    buf.split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| OsString::from_vec(a.to_vec()))
        .collect()
}

fn num(v: &OsStr, what: &str) -> i32 {
    v.to_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| fail(&format!("Invalid fd: {what}")))
}

struct Parsed {
    status_fd: Option<i32>,
    sync_fd: Option<i32>,
    env_ops: Vec<(OsString, Option<OsString>)>,
    clearenv: bool,
    chdir: Option<PathBuf>,
    binds: BTreeMap<PathBuf, PathBuf>,
    die_with_parent: bool,
    new_session: bool,
}

fn parse(opts: &[OsString]) -> Parsed {
    let mut p = Parsed {
        status_fd: None,
        sync_fd: None,
        env_ops: Vec::new(),
        clearenv: false,
        chdir: None,
        binds: BTreeMap::new(),
        die_with_parent: false,
        new_session: false,
    };
    let mut i = 0;
    while i < opts.len() {
        let o = opts[i].to_str().unwrap_or("");
        if !o.starts_with("--") {
            fail(&format!("Unknown option {o}"));
        }
        let n = option_arity(&opts[i]);
        if n == 0 && !FLAGS.contains(&o) {
            fail(&format!("Unknown option {o}"));
        }
        if n > 0 && i + n >= opts.len() {
            fail(&format!("{o} takes {n} arguments"));
        }
        let v = &opts[i + 1..=i + n];
        match o {
            "--perms" | "--size" => {
                let next = opts.get(i + 2).and_then(|n| n.to_str()).unwrap_or("");
                let ok = if o == "--size" {
                    next == "--tmpfs" || next == "--perms"
                } else {
                    PERMS_TARGETS.contains(&next) || next == "--size"
                };
                if !ok {
                    fail(&format!(
                        "{o} must be followed by an option that creates a file"
                    ));
                }
            }
            "--args" => fail("--args is not allowed inside --args"),
            "--json-status-fd" => p.status_fd = Some(num(&v[0], o)),
            "--sync-fd" => p.sync_fd = Some(num(&v[0], o)),
            "--setenv" => p.env_ops.push((v[0].clone(), Some(v[1].clone()))),
            "--unsetenv" => p.env_ops.push((v[0].clone(), None)),
            "--clearenv" => p.clearenv = true,
            "--chdir" => p.chdir = Some(PathBuf::from(&v[0])),
            "--die-with-parent" => p.die_with_parent = true,
            "--new-session" => p.new_session = true,
            "--bind" | "--bind-try" | "--ro-bind" | "--ro-bind-try" | "--dev-bind"
            | "--dev-bind-try" => {
                p.binds.insert(PathBuf::from(&v[1]), PathBuf::from(&v[0]));
            }
            _ => {}
        }
        i += 1 + n;
    }
    p
}

/// Map a sandbox path to the host path through the bind mounts (longest destination first).
fn host_path(binds: &BTreeMap<PathBuf, PathBuf>, p: &Path) -> PathBuf {
    let mut best: Option<(&PathBuf, &PathBuf)> = None;
    for (dst, src) in binds {
        if p.starts_with(dst)
            && best.is_none_or(|(d, _)| dst.as_os_str().len() > d.as_os_str().len())
        {
            best = Some((dst, src));
        }
    }
    match best {
        Some((dst, src)) if dst != Path::new("/") => match p.strip_prefix(dst) {
            Ok(rest) if !rest.as_os_str().is_empty() => src.join(rest),
            _ => src.clone(),
        },
        _ => p.to_path_buf(),
    }
}

fn own_pid_ns() -> u64 {
    std::fs::read_link("/proc/self/ns/pid")
        .ok()
        .and_then(|t| {
            let s = t.to_string_lossy().into_owned();
            s.strip_prefix("pid:[")?.strip_suffix(']')?.parse().ok()
        })
        .unwrap_or(0)
}

fn set_cloexec(fd: i32) {
    // SAFETY: fcntl on an integer descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

fn main() {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    if argv.first().is_some_and(|a| a == "--version") {
        println!("bubblewrap 0.12.0");
        return;
    }
    let mut opts = Vec::new();
    let mut i = 0;
    while i < argv.len() && argv[i] != "--" {
        if argv[i] == "--args" {
            let fd = argv
                .get(i + 1)
                .map(|v| num(v, "--args"))
                .unwrap_or_else(|| fail("--args takes 1 argument"));
            set_cloexec(fd);
            opts.extend(read_args_fd(fd));
            i += 2;
        } else {
            opts.push(argv[i].clone());
            i += 1;
        }
    }
    let command: Vec<OsString> = argv.get(i + 1..).map(<[_]>::to_vec).unwrap_or_default();
    if command.is_empty() {
        fail("No command specified");
    }
    let p = parse(&opts);

    let program = host_path(&p.binds, Path::new(&command[0]));
    let mut cmd = Command::new(&program);
    cmd.args(&command[1..]);
    if p.clearenv {
        cmd.env_clear();
    }
    for (k, v) in &p.env_ops {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    if let Some(d) = &p.chdir
        && d.is_dir()
    {
        cmd.current_dir(d);
    }
    if let Some(fd) = p.status_fd {
        set_cloexec(fd);
    }
    let (die, session) = (p.die_with_parent, p.new_session);
    // SAFETY: only async-signal-safe calls (prctl, setsid) in the child.
    unsafe {
        cmd.pre_exec(move || {
            if die {
                libc::prctl(
                    libc::PR_SET_PDEATHSIG,
                    libc::SIGKILL as libc::c_ulong,
                    0,
                    0,
                    0,
                );
            }
            if session {
                libc::setsid();
            }
            Ok(())
        });
    }
    // SAFETY: the status descriptor was handed to us; it is owned here from now on.
    let mut status = p.status_fd.map(|fd| unsafe { File::from_raw_fd(fd) });
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => fail(&format!(
            "execvp {}: {e}",
            String::from_utf8_lossy(program.as_os_str().as_bytes())
        )),
    };
    if let Some(f) = status.as_mut() {
        let _ = writeln!(
            f,
            "{{ \"child-pid\": {}, \"pid-namespace\": {} }}",
            child.id(),
            own_pid_ns()
        );
    }
    let code = match child.wait() {
        Ok(s) => s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0)),
        Err(_) => 1,
    };
    if let Some(f) = status.as_mut() {
        let _ = writeln!(f, "{{ \"exit-code\": {code} }}");
    }
    // The sync descriptor (if any) closes with this process; descendants that inherited it
    // keep it open until they exit, like the processes of a real sandbox.
    let _ = p.sync_fd;
    exit(code);
}
