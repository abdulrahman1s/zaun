//! The overlay process (ADR-0037): `zaun-overlay` under Bubblewrap, spoken to over its
//! standard input and output.
//!
//! - **No socket and no network.** Games cannot reach it: it has pipes to Zaun only, and
//!   its network namespace is empty.
//! - **Sandbox.** The system's programs and libraries read-only, private `/dev` (with the GPU
//!   nodes), `/proc` and `/tmp`, `/sys` read-only for the graphics drivers, and the
//!   compositor's socket. No home folder, no data folder, nothing writable but `/tmp`.
//! - **The compositor's own socket**, not a security-context one (ADR-0029): compositors hide
//!   the layer shell from security-context clients, and the overlay needs it.
//! - **Threads.** A writer thread owns the input, a reader thread owns the output and the
//!   child. Dropping the overlay closes its input; it exits, and after [`STOP_GRACE`] it is
//!   killed.
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use super::protocol::{self, FromOverlay, LineError, ToOverlay};
use super::seam::{Overlay, OverlayEvent, OverlayFactory, OverlaySink};
use crate::error::Result;
use crate::session::{PipedHelper, spawn_piped_helper};

/// How long a stopping overlay may take before it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(2);
/// The overlay's log, inside Zaun's logs folder.
pub const LOG_NAME: &str = "overlay.log";
/// The global a compositor offers layer surfaces through (none on GNOME).
pub const LAYER_SHELL: &str = "zwlr_layer_shell_v1";
/// Where the compositor's socket appears inside the sandbox (`XDG_RUNTIME_DIR` = `/tmp`).
pub const SOCKET_INSIDE: &str = "/tmp/wayland-0";

/// Read-only binds: programs, libraries, system configuration and the graphics drivers.
/// `-try`: whatever does not exist on this system is skipped.
const SYSTEM_BINDS: &[&str] = &[
    "/usr",
    "/lib",
    "/lib64",
    "/lib32",
    "/bin",
    "/sbin",
    "/etc",
    "/opt",
    "/nix/store",
    "/run/current-system",
    "/run/opengl-driver",
    "/run/opengl-driver-32",
    // Mesa and the Vulkan loader find the GPU through these.
    "/sys/dev",
    "/sys/devices",
    "/sys/bus",
    "/sys/class",
];

/// Starts `zaun-overlay`.
#[derive(Clone, Debug)]
pub struct HelperOverlayFactory {
    pub helper: PathBuf,
    /// `bwrap`; `None` = run unsandboxed (development only).
    pub bwrap: Option<PathBuf>,
    /// The compositor's socket (absolute); `None` = no Wayland session, no overlay.
    pub wayland: Option<PathBuf>,
    /// GPU nodes outside `/dev/dri` (NVIDIA's).
    pub gpu_nodes: Vec<PathBuf>,
    /// `LD_LIBRARY_PATH` for the overlay (the graphics and Wayland libraries on NixOS).
    pub library_path: Option<OsString>,
    pub log: PathBuf,
}

impl HelperOverlayFactory {
    /// The command line that starts the overlay: `(program, args)`.
    pub fn command(&self, socket: &Path) -> (PathBuf, Vec<OsString>) {
        let Some(bwrap) = &self.bwrap else {
            return (self.helper.clone(), Vec::new());
        };
        // No `--share-net`: the overlay needs no network.
        let mut a: Vec<OsString> = [
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/tmp",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        let mut bind = |flag: &str, src: &Path, dst: &Path| {
            a.extend([flag.into(), src.into(), dst.into()]);
        };
        for p in SYSTEM_BINDS {
            bind("--ro-bind-try", Path::new(p), Path::new(p));
        }
        bind(
            "--dev-bind-try",
            Path::new("/dev/dri"),
            Path::new("/dev/dri"),
        );
        for n in &self.gpu_nodes {
            bind("--dev-bind-try", n, n);
        }
        bind("--ro-bind", &self.helper, &self.helper);
        bind("--ro-bind", socket, Path::new(SOCKET_INSIDE));
        a.extend(["--chdir".into(), "/tmp".into(), "--".into()]);
        a.push(self.helper.clone().into());
        (bwrap.clone(), a)
    }

    fn env(&self, socket: &Path) -> Vec<(OsString, OsString)> {
        let (runtime, display): (OsString, OsString) = if self.bwrap.is_some() {
            ("/tmp".into(), "wayland-0".into())
        } else {
            let dir = socket.parent().unwrap_or(Path::new("/"));
            (dir.into(), socket.into())
        };
        let mut env: Vec<(OsString, OsString)> = vec![
            ("HOME".into(), "/tmp".into()),
            ("LANG".into(), "C.UTF-8".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("XDG_RUNTIME_DIR".into(), runtime),
            ("WAYLAND_DISPLAY".into(), display),
            // A session-wide MangoHud would draw its own overlay into ours.
            ("DISABLE_MANGOHUD".into(), "1".into()),
        ];
        if let Some(p) = &self.library_path {
            env.push(("LD_LIBRARY_PATH".into(), p.clone()));
        }
        env
    }
}

impl OverlayFactory for HelperOverlayFactory {
    fn available(&self) -> bool {
        self.helper.is_file() && self.wayland.is_some()
    }

    fn has_layer_shell(&self) -> Option<bool> {
        let socket = self.wayland.as_deref()?;
        match crate::host::wayland::globals(socket) {
            Ok(g) => Some(g.iter().any(|i| i == LAYER_SHELL)),
            Err(e) => {
                tracing::info!("overlay: cannot list the compositor's globals: {e}");
                None
            }
        }
    }

    fn start(&self, sink: OverlaySink) -> Result<Box<dyn Overlay>> {
        let Some(socket) = self.wayland.as_deref().filter(|_| self.helper.is_file()) else {
            bail!(
                ChtUnavailable,
                detail = "no overlay helper or no Wayland display"
            );
        };
        if self.bwrap.is_none() {
            tracing::warn!("overlay: bwrap not found; the overlay runs without a sandbox");
        }
        let (program, args) = self.command(socket);
        let env = self.env(socket);
        let mut child = spawn_piped_helper(&PipedHelper {
            program: &program,
            args: &args,
            env: &env,
            log: &self.log,
        })
        .map_err(|e| err!(ChtUnavailable).with_source(e))?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            bail!(ChtUnavailable);
        };
        let pid = child.id();
        let (tx, rx) = channel::<String>();
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(f)
                .map_err(|e| err!(IntTaskFailed).with_source(e))
        };
        spawn(
            "overlay-write",
            Box::new(move || {
                for line in rx {
                    if writeln!(stdin, "{line}")
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            }),
        )?;
        let exited = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&exited);
        spawn(
            "overlay-read",
            Box::new(move || {
                let mut ready = false;
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    match read_line(&line) {
                        Some(OverlayEvent::Said(FromOverlay::Ready)) => {
                            ready = true;
                            sink(OverlayEvent::Said(FromOverlay::Ready));
                        }
                        Some(e) => sink(e),
                        None => {}
                    }
                }
                let status = child.wait().ok().and_then(|s| s.code());
                done.store(true, Ordering::SeqCst);
                sink(OverlayEvent::Exited { status, ready });
            }),
        )?;
        Ok(Box::new(HelperOverlay {
            tx: Some(tx),
            pid,
            exited,
        }))
    }
}

/// What one line from the overlay means: a message, a version the app does not speak, or
/// nothing (malformed: logged and dropped).
pub fn read_line(line: &str) -> Option<OverlayEvent> {
    match protocol::decode::<FromOverlay>(line) {
        Ok(m) => Some(OverlayEvent::Said(m)),
        Err(LineError::Version(v)) => Some(OverlayEvent::Incompatible(v)),
        Err(e @ LineError::Malformed(_)) => {
            tracing::warn!(error = %e, "overlay: line dropped");
            None
        }
    }
}

struct HelperOverlay {
    tx: Option<Sender<String>>,
    pid: u32,
    exited: Arc<AtomicBool>,
}

impl Overlay for HelperOverlay {
    fn send(&self, msg: &ToOverlay) {
        if let Some(tx) = &self.tx {
            // The writer ended with the process; its exit is reported by the reader.
            let _ = tx.send(protocol::encode(msg));
        }
    }
}

impl Drop for HelperOverlay {
    fn drop(&mut self) {
        // Closing the channel ends the writer, which closes the overlay's input.
        self.tx = None;
        let exited = Arc::clone(&self.exited);
        let pid = self.pid;
        let spawned = std::thread::Builder::new()
            .name("overlay-stop".into())
            .spawn(move || {
                std::thread::sleep(STOP_GRACE);
                if !exited.load(Ordering::SeqCst)
                    && let Some(pid) = i32::try_from(pid)
                        .ok()
                        .and_then(rustix::process::Pid::from_raw)
                {
                    // The reader thread still holds the child unreaped, so the PID is ours.
                    let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "overlay: stop timer not started");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn factory(bwrap: Option<&str>) -> HelperOverlayFactory {
        HelperOverlayFactory {
            helper: "/opt/zaun/libexec/zaun-overlay".into(),
            bwrap: bwrap.map(PathBuf::from),
            wayland: Some("/run/user/1000/wayland-1".into()),
            gpu_nodes: vec!["/dev/nvidia0".into()],
            library_path: Some("/nix/store/x-wayland/lib".into()),
            log: "/tmp/overlay.log".into(),
        }
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_overlay_has_no_network_no_home_and_nothing_writable_but_its_tmp() {
        let f = factory(Some("/usr/bin/bwrap"));
        let (program, args) = f.command(Path::new("/run/user/1000/wayland-1"));
        assert_eq!(program, PathBuf::from("/usr/bin/bwrap"));
        let args = strings(&args);
        assert!(args.iter().any(|a| a == "--unshare-all"));
        assert!(!args.iter().any(|a| a == "--share-net"));
        assert!(!args.iter().any(|a| a == "--bind" || a == "--bind-try"));
        assert!(!args.iter().any(|a| a.starts_with("/home")));
        for flag in ["--die-with-parent", "--new-session"] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        // The compositor's socket, read-only, at a fixed place inside.
        assert!(args.windows(3).any(|w| w[0] == "--ro-bind"
            && w[1] == "/run/user/1000/wayland-1"
            && w[2] == SOCKET_INSIDE));
        // The GPU.
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--dev-bind-try" && w[1] == "/dev/dri")
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--dev-bind-try" && w[1] == "/dev/nvidia0")
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("/opt/zaun/libexec/zaun-overlay")
        );
    }

    #[test]
    fn the_environment_names_the_socket_inside_and_turns_mangohud_off() {
        let f = factory(Some("/usr/bin/bwrap"));
        let env = f.env(Path::new("/run/user/1000/wayland-1"));
        let get = |k: &str| {
            env.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        assert_eq!(get("WAYLAND_DISPLAY").as_deref(), Some("wayland-0"));
        assert_eq!(get("XDG_RUNTIME_DIR").as_deref(), Some("/tmp"));
        assert_eq!(get("DISABLE_MANGOHUD").as_deref(), Some("1"));
        assert_eq!(
            get("LD_LIBRARY_PATH").as_deref(),
            Some("/nix/store/x-wayland/lib")
        );
        let bare = factory(None).env(Path::new("/run/user/1000/wayland-1"));
        assert!(
            bare.iter()
                .any(|(k, v)| k == "WAYLAND_DISPLAY" && v == "/run/user/1000/wayland-1")
        );
    }

    #[test]
    fn without_bwrap_the_overlay_runs_directly() {
        let (program, args) = factory(None).command(Path::new("/s"));
        assert_eq!(program, PathBuf::from("/opt/zaun/libexec/zaun-overlay"));
        assert!(args.is_empty());
    }

    #[test]
    fn without_a_wayland_display_there_is_no_overlay() {
        let mut f = factory(None);
        f.wayland = None;
        assert!(!f.available());
        let sink: OverlaySink = Arc::new(|_| {});
        assert!(f.start(sink).is_err());
    }

    #[test]
    fn lines_from_the_overlay_are_messages_a_version_or_dropped() {
        assert_eq!(
            read_line(r#"{"v":2,"t":"end_call"}"#),
            Some(OverlayEvent::Said(FromOverlay::EndCall))
        );
        assert_eq!(
            read_line(r#"{"v":3,"t":"ready"}"#),
            Some(OverlayEvent::Incompatible(3))
        );
        assert_eq!(read_line("garbage"), None);
    }
}
