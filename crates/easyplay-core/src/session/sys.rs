//! Process primitives: signals by [`ProcIdentity`], and the code that runs between `fork`
//! and `exec` (descriptor placement, `PR_SET_PDEATHSIG`, `setsid`, the detaching double
//! fork). The only file of the session module that contains `unsafe` (ARCHITECTURE §3
//! rule 5); every block carries a `// SAFETY:` comment.
//!
//! Children are started through `std::process::Command`; the hooks below are installed with
//! `CommandExt::pre_exec` and therefore run in the forked child, where only
//! async-signal-safe calls are allowed. Everything they touch is prepared before the fork.

#![allow(unsafe_code)]

use std::io;
use std::os::fd::RawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;

use rustix::process::{Pid, Signal};

use crate::procfs::{ProcIdentity, ProcSource};

/// A signal EasyPlay sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sig {
    /// `SIGTERM`: graceful stop.
    Term,
    /// `SIGUSR1`: force stop (to a supervisor).
    Usr1,
    /// `SIGKILL`.
    Kill,
}

impl Sig {
    fn rustix(self) -> Signal {
        match self {
            Sig::Term => Signal::TERM,
            Sig::Usr1 => Signal::USR1,
            Sig::Kill => Signal::KILL,
        }
    }
}

/// Why a signal was not delivered.
#[derive(Debug, thiserror::Error)]
pub enum SignalError {
    /// The process is gone, or the PID now belongs to another process.
    #[error("the process is gone")]
    Gone,
    /// `EPERM` (`EP-SES-021`).
    #[error("not permitted to signal the process")]
    Denied,
    /// Another error.
    #[error("signal failed: {0}")]
    Other(io::Error),
}

/// Send `sig` to the process with identity `id`, verifying PID **and** start time
/// immediately before sending (PROCESS_LIFECYCLE §10: PID reuse cannot misdirect a signal).
pub fn signal_identity(
    proc: &dyn ProcSource,
    id: ProcIdentity,
    sig: Sig,
) -> Result<(), SignalError> {
    match proc.is_alive(id) {
        Ok(true) => {}
        Ok(false) => return Err(SignalError::Gone),
        Err(e) => return Err(SignalError::Other(io::Error::other(e.to_string()))),
    }
    signal_pid(id.pid, sig)
}

/// Send `sig` to `pid` without an identity check. Only for processes this process started
/// and has not reaped yet (their PID cannot be reused), and for members of a sandbox that
/// were listed a moment ago by namespace.
pub fn signal_pid(pid: u32, sig: Sig) -> Result<(), SignalError> {
    let raw = i32::try_from(pid).map_err(|_| SignalError::Gone)?;
    let Some(pid) = Pid::from_raw(raw) else {
        return Err(SignalError::Gone);
    };
    match rustix::process::kill_process(pid, sig.rustix()) {
        Ok(()) => Ok(()),
        Err(rustix::io::Errno::SRCH) => Err(SignalError::Gone),
        Err(rustix::io::Errno::PERM) => Err(SignalError::Denied),
        Err(e) => Err(SignalError::Other(e.into())),
    }
}

/// Descriptor number from which [`place_fds`] parks the sources before moving them.
const PARK_BASE: RawFd = 64;

/// Close-on-exec for every descriptor from `first` on: `close_range(…, CLOSE_RANGE_CLOEXEC)`
/// (Linux 5.11), falling back to `fcntl` over the soft limit (capped). Async-signal-safe.
fn cloexec_from(first: RawFd) {
    const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;
    // SAFETY: plain system calls without memory arguments.
    unsafe {
        let r = libc::syscall(
            libc::SYS_close_range,
            first as libc::c_uint,
            libc::c_uint::MAX,
            CLOSE_RANGE_CLOEXEC,
        );
        if r == 0 {
            return;
        }
        for fd in first..4096 {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
}

/// In the child: move each `(source, target)` descriptor to `target` (without close-on-exec)
/// and mark every other descriptor from 3 on close-on-exec. Sources are first parked above
/// [`PARK_BASE`] so that a source equal to another pair's target is not overwritten.
/// Async-signal-safe; `pairs` is prepared before the fork.
fn place_fds(pairs: &[(RawFd, RawFd)], parked: &mut [RawFd]) -> io::Result<()> {
    for (i, (src, _)) in pairs.iter().enumerate() {
        // SAFETY: fcntl(F_DUPFD_CLOEXEC) has no memory arguments.
        let p = unsafe { libc::fcntl(*src, libc::F_DUPFD_CLOEXEC, PARK_BASE) };
        if p < 0 {
            return Err(io::Error::last_os_error());
        }
        parked[i] = p;
    }
    cloexec_from(3);
    for (i, (_, dst)) in pairs.iter().enumerate() {
        // SAFETY: dup2 has no memory arguments; it clears close-on-exec on `dst`.
        if unsafe { libc::dup2(parked[i], *dst) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Restore default dispositions of the signals the supervisor handles or ignores and
/// unblock everything. Async-signal-safe.
fn reset_signals() {
    // SAFETY: sigemptyset fully initialises the zeroed set; signal/sigprocmask receive valid
    // arguments.
    unsafe {
        for sig in [
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGHUP,
            libc::SIGUSR1,
            libc::SIGCHLD,
            libc::SIGPIPE,
        ] {
            libc::signal(sig, libc::SIG_DFL);
        }
        let mut empty: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut empty);
        libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
    }
}

/// Configure `cmd` to start `bwrap` (BUBBLEWRAP_SANDBOX §8): descriptors placed exactly as
/// `pairs` says and every other one closed at `exec`; `PR_SET_PDEATHSIG(SIGKILL)` (the child
/// dies if the spawning thread dies, and exits at once if the parent is already gone); signal
/// dispositions and mask reset; a new session.
pub fn configure_sandbox_child(cmd: &mut Command, pairs: Vec<(RawFd, RawFd)>) {
    // SAFETY: getpid has no preconditions; the value is compared in the child with getppid.
    let me = unsafe { libc::getpid() };
    let mut parked = vec![-1 as RawFd; pairs.len()];
    // SAFETY: the hook only performs async-signal-safe system calls (prctl, getppid, _exit,
    // fcntl, dup2, syscall(close_range), signal, sigprocmask, setsid) on memory allocated
    // before the fork (`pairs`, `parked`).
    unsafe {
        cmd.pre_exec(move || {
            libc::prctl(
                libc::PR_SET_PDEATHSIG,
                libc::SIGKILL as libc::c_ulong,
                0,
                0,
                0,
            );
            if libc::getppid() != me {
                // The supervisor died between fork and prctl.
                libc::_exit(1);
            }
            place_fds(&pairs, &mut parked)?;
            reset_signals();
            libc::setsid();
            Ok(())
        });
    }
}

/// Configure `cmd` so that the started program is **not** a child of this process
/// (PROCESS_LIFECYCLE §2 step 4): in the child `setsid`, `fork` again and let the
/// intermediate exit at once; the grandchild, reparented to the session's subreaper or init,
/// execs the program. Every descriptor from 3 on is closed at `exec`.
///
/// `Command::spawn` returns once the grandchild has exec'd (the standard library's
/// close-on-exec error pipe is shared by both); the returned `Child` is the intermediate,
/// which the caller must `wait` for. The grandchild's PID is not known to the caller.
pub fn configure_detached(cmd: &mut Command) {
    // SAFETY: the hook only performs async-signal-safe calls (setsid, fork, _exit, syscall,
    // fcntl, signal, sigprocmask) and allocates nothing.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            match libc::fork() {
                -1 => return Err(io::Error::last_os_error()),
                0 => {}
                _ => libc::_exit(0),
            }
            cloexec_from(3);
            reset_signals();
            Ok(())
        });
    }
}

/// The calling process's identity (PID and start time).
pub fn own_identity(proc: &dyn ProcSource) -> Option<ProcIdentity> {
    let pid = std::process::id();
    proc.entry(pid).ok().flatten().map(|e| e.identity())
}

/// Whether `fd` is readable or hung up within `timeout_ms` (`-1` blocks), for several
/// descriptors at once. Returns per descriptor `(readable, hung_up)`.
pub fn poll_fds(fds: &[RawFd], timeout_ms: i32) -> io::Result<Vec<(bool, bool)>> {
    let mut pfds: Vec<libc::pollfd> = fds
        .iter()
        .map(|fd| libc::pollfd {
            fd: *fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    // SAFETY: `pfds` is a valid array of `pfds.len()` initialised pollfd structures.
    let r = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
    if r < 0 {
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::Interrupted {
            return Ok(vec![(false, false); fds.len()]);
        }
        return Err(e);
    }
    Ok(pfds
        .iter()
        .map(|p| {
            (
                p.revents & libc::POLLIN != 0,
                p.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0,
            )
        })
        .collect())
}
