//! Executing a rendered sandbox (PROC-001, BUBBLEWRAP_SANDBOX §3 and §8): the host actions
//! that precede it and the `bwrap` process itself.
//!
//! [`RealSpawner`] creates the sealed memfd payloads, places every descriptor at the number
//! the plan's options refer to (0 `/dev/null`; 1 and 2 the log pipe; 3 `--args`; 4
//! `--json-status-fd`; 5 `--sync-fd`; 6 the init shim's status; 7 seccomp; 8… data),
//! closes all others at `exec`, and runs `bwrap --args 3 -- <command>` with the fixed
//! environment `PATH=/usr/bin:/bin LANG=C.UTF-8` (`session::sys` holds the `unsafe` part).

use std::ffi::OsString;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, FsErrorKind};
use crate::ids::GameId;
use crate::launch::{FdKind, PlanSandbox};
use crate::sandbox::HostAction;
use crate::sandbox::fd::sealed_memfd;
use crate::sandbox::plan::{FD_ARGS, FD_INIT_STATUS, FD_JSON_STATUS, FD_SYNC};
use crate::sandbox::seccomp;

use super::lock::PrefixLock;
use super::sys;

/// The supervisor's ends of the sandbox's descriptors are the read ends; these are the write
/// ends handed to the child (PROCESS_LIFECYCLE §12).
#[derive(Debug)]
pub struct SandboxIo {
    /// stdout and stderr.
    pub log: OwnedFd,
    /// `--json-status-fd`.
    pub status: OwnedFd,
    /// The init shim's `--status-fd`.
    pub init_status: OwnedFd,
    /// `--sync-fd`.
    pub sync: OwnedFd,
}

/// The `bwrap` monitor process.
pub trait SandboxChild: Send {
    /// Its PID.
    fn monitor_pid(&self) -> u32;
    /// Its exit status (exit code, or 128 + signal) once it has exited.
    fn try_wait(&mut self) -> Result<Option<i32>, Error>;
    /// `SIGKILL` it.
    fn kill_monitor(&mut self) -> Result<(), Error>;
}

/// Starts sandboxes.
pub trait SandboxSpawner: Send {
    /// Start `bwrap` at `bwrap` with the rendered `sandbox`.
    fn spawn(
        &mut self,
        bwrap: &Path,
        sandbox: &PlanSandbox,
        io: SandboxIo,
    ) -> Result<Box<dyn SandboxChild>, Error>;
}

/// The environment of `bwrap` itself (BUBBLEWRAP_SANDBOX §3).
pub fn bwrap_env() -> Vec<(OsString, OsString)> {
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LANG".into(), "C.UTF-8".into()),
    ]
}

/// The real spawner.
#[derive(Debug, Default)]
pub struct RealSpawner;

struct RealChild {
    child: Child,
}

fn status_code(s: ExitStatus) -> i32 {
    s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0))
}

impl SandboxChild for RealChild {
    fn monitor_pid(&self) -> u32 {
        self.child.id()
    }
    fn try_wait(&mut self) -> Result<Option<i32>, Error> {
        self.child
            .try_wait()
            .map(|s| s.map(status_code))
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))
    }
    fn kill_monitor(&mut self) -> Result<(), Error> {
        match self.child.kill() {
            Ok(()) => Ok(()),
            // Already reaped or exited.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(e) => Err(Error::new(ErrorCode::IntTaskFailed).with_source(e)),
        }
    }
}

fn setup_err(what: &'static str, e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Error {
    Error::new(ErrorCode::SbxSetupFailed)
        .with("step", what)
        .with_source(e)
}

/// Owned payload descriptors and the `(source, target)` placement of every descriptor.
pub type DescriptorTable = (Vec<OwnedFd>, Vec<(RawFd, RawFd)>);

/// The descriptor table of one launch: `(source, target)` pairs, plus the owned sources
/// (kept alive until the child has been started).
pub fn descriptor_table(sandbox: &PlanSandbox, io: &SandboxIo) -> Result<DescriptorTable, Error> {
    let mut owned: Vec<OwnedFd> = Vec::new();
    let mut targets: Vec<RawFd> = Vec::new();
    let mut payload = Vec::new();
    for a in &sandbox.args {
        payload.extend_from_slice(std::os::unix::ffi::OsStrExt::as_bytes(a.as_os_str()));
        payload.push(0);
    }
    owned.push(sealed_memfd("easyplay-args", &payload).map_err(|e| setup_err("memfd", e))?);
    targets.push(FD_ARGS);
    for fd in &sandbox.fds {
        if [FD_ARGS, FD_JSON_STATUS, FD_SYNC, FD_INIT_STATUS].contains(&fd.fd) || fd.fd < 3 {
            return Err(Error::new(ErrorCode::SbxPlanInvariant).with(
                "detail",
                format!("payload on reserved descriptor {}", fd.fd),
            ));
        }
        let mut content = fd
            .content()
            .map_err(|e| Error::new(ErrorCode::SbxPlanInvariant).with_source(e))?;
        let name = if fd.kind == FdKind::Seccomp {
            // The plan records only the descriptor; the program is built (once per process)
            // for the profile the rendered options imply (BUBBLEWRAP_SANDBOX §9).
            if content.is_empty() {
                content = seccomp::program(seccomp::Profile::for_args(&sandbox.args))
                    .map_err(|e| {
                        Error::new(ErrorCode::SbxSetupFailed)
                            .with(
                                "detail",
                                "the system-call filter is not available in this build",
                            )
                            .with_source(e)
                    })?
                    .to_vec();
            }
            "easyplay-seccomp"
        } else {
            "easyplay-data"
        };
        owned.push(sealed_memfd(name, &content).map_err(|e| setup_err("memfd", e))?);
        targets.push(fd.fd);
    }
    let mut pairs: Vec<(RawFd, RawFd)> = owned
        .iter()
        .zip(&targets)
        .map(|(o, t)| (o.as_raw_fd(), *t))
        .collect();
    pairs.push((io.status.as_raw_fd(), FD_JSON_STATUS));
    pairs.push((io.sync.as_raw_fd(), FD_SYNC));
    pairs.push((io.init_status.as_raw_fd(), FD_INIT_STATUS));
    let mut seen: Vec<RawFd> = pairs.iter().map(|p| p.1).collect();
    seen.sort_unstable();
    if seen.windows(2).any(|w| w[0] == w[1]) {
        return Err(Error::new(ErrorCode::SbxPlanInvariant).with("detail", "duplicate descriptor"));
    }
    Ok((owned, pairs))
}

impl SandboxSpawner for RealSpawner {
    #[allow(clippy::disallowed_methods)] // session::spawn is one of the two places that spawn.
    fn spawn(
        &mut self,
        bwrap: &Path,
        sandbox: &PlanSandbox,
        io: SandboxIo,
    ) -> Result<Box<dyn SandboxChild>, Error> {
        crate::ui_thread::assert_not_ui_thread("SandboxSpawner::spawn");
        let (owned, pairs) = descriptor_table(sandbox, &io)?;
        let log_out = io.log.try_clone().map_err(|e| setup_err("descriptor", e))?;
        let mut cmd = Command::new(bwrap);
        cmd.arg("--args")
            .arg(FD_ARGS.to_string())
            .arg("--")
            .args(&sandbox.command)
            .env_clear()
            .envs(bwrap_env())
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(io.log));
        sys::configure_sandbox_child(&mut cmd, pairs);
        let child = cmd.spawn().map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                ErrorCode::HostBwrapNotFound
            } else {
                ErrorCode::SbxSetupFailed
            };
            Error::new(code)
                .with("path", bwrap.display())
                .with_source(e)
        })?;
        // The write ends and payloads now live in the child only.
        drop(owned);
        drop(io.status);
        drop(io.sync);
        drop(io.init_status);
        Ok(Box::new(RealChild { child }))
    }
}

// ---- host actions ---------------------------------------------------------------------------

/// What the host actions need besides the filesystem.
pub struct HostActionCtx<'a> {
    /// Filesystem.
    pub fs: &'a dyn Fs,
    /// This user's ID (owner check of existing directories).
    pub uid: u32,
    /// Locks taken so far; `AcquireLock` adds to it (the supervisor's prefix lock is taken
    /// before the row is inserted, so the action normally finds it here).
    pub locks: &'a mut Vec<PrefixLock>,
    /// `MarkTainted`: set `games.prefix_tainted_at`.
    pub mark_tainted: &'a mut dyn FnMut(GameId) -> Result<(), Error>,
}

fn create_dir(fs: &dyn Fs, uid: u32, path: &Path, mode: u32) -> Result<(), Error> {
    fs.create_dir_all(path, mode)?;
    let md = fs.metadata(path)?;
    if !md.is_dir() || md.uid != uid {
        return Err(Error::new(ErrorCode::SbxSetupFailed)
            .with("path", path.display())
            .with("detail", "not a directory owned by the user"));
    }
    Ok(())
}

fn dos_devices(
    fs: &dyn Fs,
    uid: u32,
    prefix: &Path,
    links: &[crate::sandbox::plan::DosDeviceLink],
) -> Result<(), Error> {
    let dir = prefix.join("dosdevices");
    create_dir(fs, uid, &dir, 0o755)?;
    for l in links {
        let link: PathBuf = dir.join(format!("{}:", l.letter.to_ascii_lowercase()));
        match fs.read_link(&link) {
            Ok(t) if t == l.target => continue,
            Ok(_) => fs.remove_file(&link)?,
            Err(e) if e.kind == FsErrorKind::NotFound => {}
            Err(_) => {
                return Err(Error::new(ErrorCode::SbxSetupFailed)
                    .with("path", link.display())
                    .with("detail", "drive link is not a symbolic link"));
            }
        }
        fs.symlink(&l.target, &link)?;
    }
    Ok(())
}

/// Run the plan's host actions in order (BUBBLEWRAP_SANDBOX §8). Each is idempotent.
/// `WriteMemfd` is done by the spawner and is a no-op here.
pub fn run_host_actions(actions: &[HostAction], ctx: &mut HostActionCtx<'_>) -> Result<(), Error> {
    for a in actions {
        match a {
            HostAction::CreateDir { path, mode } => create_dir(ctx.fs, ctx.uid, path, *mode)?,
            HostAction::AcquireLock { path } => {
                if !ctx.locks.iter().any(|l| l.path() == path.as_path()) {
                    ctx.locks.push(PrefixLock::try_acquire(path)?);
                }
            }
            HostAction::WriteMemfd { .. } => {}
            HostAction::EnsureDosDevices { prefix, links } => {
                dos_devices(ctx.fs, ctx.uid, prefix, links)?;
            }
            HostAction::MarkTainted { game } => (ctx.mark_tainted)(*game)?,
        }
    }
    Ok(())
}
