//! `easyplay-init`: the first process inside every sandbox (PROCESS_LIFECYCLE §4).
//!
//! `easyplay-init [--status-fd N] -- PROGRAM [ARG…]`
//!
//! Becomes a child subreaper, blocks `SIGTERM`/`SIGINT`/`SIGHUP`/`SIGCHLD` and reads them
//! synchronously, forks and execs the program, and reaps every process until none is left,
//! so "the session ends when the sandbox is empty" holds whatever the program does. A stop
//! signal is relayed to every process in the PID namespace with `kill(-1, sig)`. Status lines
//! (`started <pid>`, `main-exit <status>`, `stop-requested <signo>`, `exec-failed <errno>`,
//! `empty`) go to the optional status descriptor. Exits with the main child's status (128 + n
//! for a signal). No heap allocation after start-up, no configuration, no environment or file
//! access. `libc` only.
//!
//! Safety guard (not in the protocol): `kill(-1, …)` is only used when this process is PID 1
//! or 2, i.e. inside a fresh PID namespace. Run by hand outside a sandbox, a stop signal is
//! relayed to the main child only, so the shim can never signal the user's whole session.

// The shim is one of the three places allowed to use `unsafe` (ARCHITECTURE §3 rule 5).
#![allow(unsafe_code)]

use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStringExt;
use std::ptr;

/// Exit status for a usage error (distinct from the program's statuses and from 127).
const USAGE_EXIT: i32 = 125;
/// Exit status when the program could not be executed.
const EXEC_FAILED_EXIT: i32 = 127;

struct Invocation {
    status_fd: Option<libc::c_int>,
    argv: Vec<CString>,
}

fn parse(args: Vec<OsString>) -> Result<Invocation, &'static str> {
    let mut it = args.into_iter().skip(1);
    let mut status_fd = None;
    loop {
        match it.next() {
            None => return Err("missing `--` and program"),
            Some(a) if a == "--status-fd" => {
                let v = it.next().ok_or("--status-fd needs a value")?;
                let fd: libc::c_int = v
                    .to_str()
                    .and_then(|s| s.parse().ok())
                    .filter(|fd| *fd >= 0)
                    .ok_or("--status-fd needs a non-negative number")?;
                status_fd = Some(fd);
            }
            Some(a) if a == "--" => break,
            Some(_) => return Err("unexpected argument before `--`"),
        }
    }
    let argv = it
        .map(|a| CString::new(a.into_vec()).map_err(|_| "argument contains a NUL byte"))
        .collect::<Result<Vec<_>, _>>()?;
    if argv.is_empty() {
        return Err("no program after `--`");
    }
    Ok(Invocation { status_fd, argv })
}

fn errno() -> libc::c_int {
    // SAFETY: `__errno_location` returns a valid pointer to this thread's errno.
    unsafe { *libc::__errno_location() }
}

/// Write `word` or `word <num>` plus a newline, without allocating. Best effort.
fn status(fd: Option<libc::c_int>, word: &[u8], num: Option<i64>) {
    let Some(fd) = fd else { return };
    let mut buf = [0u8; 64];
    let mut len = 0;
    for &b in word.iter().take(40) {
        buf[len] = b;
        len += 1;
    }
    if let Some(n) = num {
        buf[len] = b' ';
        len += 1;
        if n < 0 {
            buf[len] = b'-';
            len += 1;
        }
        let mut digits = [0u8; 20];
        let mut d = 0;
        let mut v = n.unsigned_abs();
        loop {
            digits[d] = b'0' + (v % 10) as u8;
            d += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while d > 0 {
            d -= 1;
            buf[len] = digits[d];
            len += 1;
        }
    }
    buf[len] = b'\n';
    len += 1;
    let mut off = 0;
    while off < len {
        // SAFETY: `buf[off..len]` is initialised memory owned by this frame.
        let r = unsafe { libc::write(fd, buf[off..].as_ptr().cast(), len - off) };
        if r > 0 {
            off += r as usize;
        } else if r < 0 && errno() == libc::EINTR {
            continue;
        } else {
            // Closed pipe or bad descriptor: status lines are best effort.
            return;
        }
    }
}

fn exit_code(wstatus: libc::c_int) -> i32 {
    if libc::WIFEXITED(wstatus) {
        libc::WEXITSTATUS(wstatus)
    } else if libc::WIFSIGNALED(wstatus) {
        128 + libc::WTERMSIG(wstatus)
    } else {
        // Not reachable without WUNTRACED/WCONTINUED.
        128
    }
}

fn main() {
    let inv = match parse(std::env::args_os().collect()) {
        Ok(inv) => inv,
        Err(msg) => {
            eprintln!(
                "easyplay-init: {msg}\nusage: easyplay-init [--status-fd N] -- PROGRAM [ARG...]"
            );
            std::process::exit(USAGE_EXIT);
        }
    };
    let mut argv_ptrs: Vec<*const libc::c_char> = inv.argv.iter().map(|a| a.as_ptr()).collect();
    argv_ptrs.push(ptr::null());

    // SAFETY: plain syscalls with valid arguments; failures are tolerated (subreaper is
    // unnecessary when we are PID 1, and an invalid status descriptor disables status lines).
    let status_fd = unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        match inv.status_fd {
            Some(fd) if libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) == 0 => Some(fd),
            _ => None,
        }
    };

    // SAFETY: zeroed sigset_t values are valid inputs for sigemptyset/sigaddset, which fully
    // initialise them; sigprocmask receives valid pointers.
    let (wait_set, empty_set) = unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        let mut empty: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigemptyset(&mut empty);
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGCHLD] {
            libc::sigaddset(&mut set, sig);
        }
        libc::sigprocmask(libc::SIG_BLOCK, &set, ptr::null_mut());
        (set, empty)
    };

    // A close-on-exec pipe tells the parent whether `execvp` failed and with which errno.
    let mut pipe_fds = [-1 as libc::c_int; 2];
    // SAFETY: `pipe_fds` is a valid two-element array.
    let have_pipe = unsafe { libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0;

    // SAFETY: the child performs only async-signal-safe calls (signal, sigprocmask, close,
    // execvp, write, _exit) on memory prepared before the fork; no allocation happens there.
    let child = unsafe { libc::fork() };
    if child == 0 {
        // SAFETY: see above; all pointers were created before the fork and stay valid.
        unsafe {
            if have_pipe {
                libc::close(pipe_fds[0]);
            }
            for sig in [
                libc::SIGTERM,
                libc::SIGINT,
                libc::SIGHUP,
                libc::SIGCHLD,
                libc::SIGPIPE,
            ] {
                libc::signal(sig, libc::SIG_DFL);
            }
            libc::sigprocmask(libc::SIG_SETMASK, &empty_set, ptr::null_mut());
            libc::execvp(argv_ptrs[0], argv_ptrs.as_ptr());
            let e = *libc::__errno_location();
            if have_pipe {
                libc::write(pipe_fds[1], (&e as *const libc::c_int).cast(), 4);
            }
            libc::_exit(EXEC_FAILED_EXIT);
        }
    }
    if child < 0 {
        status(status_fd, b"exec-failed", Some(i64::from(errno())));
        std::process::exit(EXEC_FAILED_EXIT);
    }

    status(status_fd, b"started", Some(i64::from(child)));
    if have_pipe {
        let mut buf = [0u8; 4];
        let mut got = 0usize;
        // SAFETY: closing our copy of the write end; reading into a valid local buffer.
        unsafe {
            libc::close(pipe_fds[1]);
            loop {
                let r = libc::read(pipe_fds[0], buf[got..].as_mut_ptr().cast(), 4 - got);
                if r > 0 {
                    got += r as usize;
                    if got == 4 {
                        break;
                    }
                } else if r < 0 && errno() == libc::EINTR {
                    continue;
                } else {
                    break;
                }
            }
            libc::close(pipe_fds[0]);
        }
        if got == 4 {
            status(
                status_fd,
                b"exec-failed",
                Some(i64::from(i32::from_ne_bytes(buf))),
            );
        }
    }

    // SAFETY: getpid has no preconditions.
    let in_new_pid_ns = unsafe { libc::getpid() } <= 2;
    let mut main_status: Option<i32> = None;
    loop {
        // Reap everything that has exited.
        loop {
            let mut wstatus: libc::c_int = 0;
            // SAFETY: valid pointer to a local.
            let r = unsafe { libc::waitpid(-1, &mut wstatus, libc::WNOHANG) };
            if r > 0 {
                if r == child {
                    let code = exit_code(wstatus);
                    main_status = Some(code);
                    status(status_fd, b"main-exit", Some(i64::from(code)));
                }
                continue;
            }
            if r == 0 {
                break; // Children remain.
            }
            match errno() {
                libc::EINTR => continue,
                _ => {
                    // ECHILD: the sandbox is empty.
                    status(status_fd, b"empty", None);
                    std::process::exit(main_status.unwrap_or(EXEC_FAILED_EXIT));
                }
            }
        }
        // SAFETY: `wait_set` is initialised; a null siginfo pointer is allowed.
        let sig = unsafe { libc::sigwaitinfo(&wait_set, ptr::null_mut()) };
        if sig == libc::SIGTERM || sig == libc::SIGINT || sig == libc::SIGHUP {
            status(status_fd, b"stop-requested", Some(i64::from(sig)));
            // SAFETY: kill has no memory preconditions.
            unsafe {
                if in_new_pid_ns {
                    libc::kill(-1, sig);
                } else if main_status.is_none() {
                    libc::kill(child, sig);
                }
            }
        }
        // SIGCHLD or EINTR: loop and reap.
    }
}
