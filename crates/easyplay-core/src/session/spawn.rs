//! Executing a rendered sandbox (PROC-001, BUBBLEWRAP_SANDBOX §3 and §8): the host actions
//! that precede it and the `bwrap` process itself.
//!
//! [`RealSpawner`] creates the sealed memfd payloads, places every descriptor at the number
//! the plan's options refer to (0 `/dev/null`; 1 and 2 the log pipe; 3 `--args`; 4
//! `--json-status-fd`; 5 `--sync-fd`; 6 the init shim's status; 7 seccomp; 8… data),
//! closes all others at `exec`, and runs `bwrap --args 3 -- <command>` with the fixed
//! environment `PATH=/usr/bin:/bin LANG=C.UTF-8` (`session::sys` holds the `unsafe` part).
//! It also starts the session's sound server (ADR-0026) and registers the session's
//! restricted Wayland socket with the compositor (ADR-0029) before the sandbox.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::fs::{FileTypeExt as _, OpenOptionsExt as _};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, FsErrorKind};
use crate::host::wayland::{ContextRequest, create_security_context};
use crate::ids::GameId;
use crate::launch::{FdKind, PlanSandbox};
use crate::sandbox::HostAction;
use crate::sandbox::fd::sealed_memfd;
use crate::sandbox::plan::{FD_ARGS, FD_INIT_STATUS, FD_JSON_STATUS, FD_SYNC, sound_socket};
use crate::sandbox::{seccomp, sound};

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

/// What [`HostAction::StartSoundServer`] starts, once [`write_sound_config`] has written its
/// configuration.
#[derive(Clone, Copy, Debug)]
pub struct SoundServer<'a> {
    /// The `pipewire` program.
    pub program: &'a Path,
    /// Its configuration.
    pub config: &'a Path,
    /// The socket it listens on.
    pub socket: &'a Path,
    /// The host's PipeWire socket.
    pub remote: &'a Path,
    /// Its output.
    pub log: &'a Path,
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

    /// Start the session's sound server and return once its socket exists. The handle's
    /// `kill_monitor` stops it.
    fn start_sound_server(
        &mut self,
        server: SoundServer<'_>,
    ) -> Result<Box<dyn SandboxChild>, Error>;

    /// Create the session's restricted Wayland socket and register it with the compositor
    /// ([`HostAction::CreateWaylandContext`]). The compositor serves it until every write
    /// end of `close_fd`'s pipe is closed.
    fn create_wayland_context(
        &mut self,
        request: ContextRequest<'_>,
        close_fd: BorrowedFd<'_>,
    ) -> Result<(), Error>;
}

/// How long the sound server has to create its socket.
const SOUND_SERVER_TIMEOUT: Duration = Duration::from_secs(5);
/// How often its socket is looked for.
const SOUND_SERVER_POLL: Duration = Duration::from_millis(20);

fn sound_err(detail: &str, e: Option<std::io::Error>) -> Error {
    let err = Error::new(ErrorCode::SbxSetupFailed)
        .with("step", "sound server")
        .with("detail", detail);
    match e {
        Some(e) => err.with_source(e),
        None => err,
    }
}

/// Create the sound server's directory `dir` (0700, the user's) and write its configuration
/// with a fresh secret read from the kernel (ADR-0026). Returns the configuration's path.
pub fn write_sound_config(fs: &dyn Fs, uid: u32, dir: &Path) -> Result<PathBuf, Error> {
    create_dir(fs, uid, dir, 0o700)?;
    let bytes = fs.read_range(Path::new("/dev/urandom"), 0, sound::SECRET_BYTES)?;
    if bytes.len() != sound::SECRET_BYTES {
        return Err(sound_err("too few random bytes", None));
    }
    let secret = crate::model::to_hex(&bytes);
    let config = dir.join(sound::SOUND_CONFIG);
    fs.atomic_write(
        &config,
        sound::config(&sound_socket(dir), &secret).as_bytes(),
        0o600,
    )?;
    Ok(config)
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

    // session::spawn is one of the two places that spawn; it also opens the server's log.
    #[allow(clippy::disallowed_methods)]
    fn start_sound_server(
        &mut self,
        server: SoundServer<'_>,
    ) -> Result<Box<dyn SandboxChild>, Error> {
        crate::ui_thread::assert_not_ui_thread("SandboxSpawner::start_sound_server");
        let log = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(server.log)
            .map_err(|e| sound_err("log", Some(e)))?;
        let log_err = log.try_clone().map_err(|e| sound_err("log", Some(e)))?;
        let dir = server.config.parent().unwrap_or(Path::new("/"));
        let mut cmd = Command::new(server.program);
        // No user or system configuration: the file is the whole of it.
        cmd.arg("-c")
            .arg(server.config)
            .env_clear()
            .env("PIPEWIRE_REMOTE", server.remote)
            .env("PIPEWIRE_CONFIG_DIR", dir)
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        if let Some(xrd) = std::env::var_os("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", xrd);
        }
        sys::configure_sandbox_child(&mut cmd, Vec::new());
        let child = cmd.spawn().map_err(|e| sound_err("start", Some(e)))?;
        let mut child = RealChild { child };
        let polls = SOUND_SERVER_TIMEOUT.as_millis() / SOUND_SERVER_POLL.as_millis();
        for _ in 0..polls {
            if std::fs::symlink_metadata(server.socket).is_ok_and(|m| m.file_type().is_socket()) {
                return Ok(Box::new(child));
            }
            if let Some(code) = child.try_wait()? {
                return Err(sound_err(
                    &format!("exited with status {code} before listening"),
                    None,
                ));
            }
            std::thread::sleep(SOUND_SERVER_POLL);
        }
        let _ = child.kill_monitor();
        let _ = child.child.wait();
        Err(sound_err("its socket did not appear", None))
    }

    fn create_wayland_context(
        &mut self,
        request: ContextRequest<'_>,
        close_fd: BorrowedFd<'_>,
    ) -> Result<(), Error> {
        crate::ui_thread::assert_not_ui_thread("SandboxSpawner::create_wayland_context");
        create_security_context(&request, close_fd)
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
            // Started by the supervisor through the spawner, which owns processes.
            HostAction::StartSoundServer { .. } => {}
            // Created by the supervisor through the spawner: it needs the `--sync-fd` pipe.
            HostAction::CreateWaylandContext { .. } => {}
        }
    }
    Ok(())
}
