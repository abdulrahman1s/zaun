//! The SimpleX engine (ADR-0033): `easyplay-chat` (the helper that hosts `libsimplex`)
//! run under Bubblewrap, spoken to over its standard input and output.
//!
//! - **No socket.** The engine's own API server would be an unauthenticated localhost port
//!   that every game with network access could reach; the helper has pipes only.
//! - **Sandbox.** The helper sees the system's programs and libraries read-only, `/etc`
//!   read-only, a private `/tmp`, `/dev` and `/proc`, the network, and one writable folder:
//!   its data folder. Not the home folder, not EasyPlay's library or the games' prefixes.
//!   Without `bwrap` (development builds) it runs unsandboxed and says so in the log.
//! - **Threads.** A writer thread owns the helper's input; a reader thread owns its output
//!   and the child, runs the [`protocol::Codec`] and reports events. Dropping the engine
//!   closes the input: the helper closes its database and exits; after
//!   [`STOP_GRACE`] it is killed.

pub mod protocol;

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::engine::{ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, Request};
use crate::clock::Clock as _;
use crate::error::{Error, ErrorCode, Result};
use crate::session::{PipedHelper, spawn_piped_helper};
use protocol::Codec;

/// How long a stopping helper may take to close its database before it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(5);
/// The database file prefix inside the data folder.
pub const DB_PREFIX: &str = "simplex";
/// The helper's log, inside EasyPlay's logs folder.
pub const LOG_NAME: &str = "chat-engine.log";

/// Read-only binds for the helper: programs, libraries and system configuration. `-try`:
/// whatever does not exist on this system is skipped.
const SYSTEM_BINDS: &[&str] = &[
    "/usr",
    "/lib",
    "/lib64",
    "/lib32",
    "/bin",
    "/sbin",
    "/etc",
    "/nix/store",
    "/run/current-system",
    "/run/systemd/resolve",
    "/run/NetworkManager",
];

/// Starts the helper.
#[derive(Clone, Debug)]
pub struct SimplexFactory {
    /// `easyplay-chat`.
    pub helper: PathBuf,
    /// `libsimplex.so` (its Haskell libraries beside it).
    pub library: PathBuf,
    /// `bwrap`; `None` = run unsandboxed (development only).
    pub bwrap: Option<PathBuf>,
    /// `LD_LIBRARY_PATH` for the helper (libraries `libsimplex` needs that are not where
    /// the system's loader looks, e.g. on NixOS).
    pub library_path: Option<OsString>,
    /// The helper's log file.
    pub log: PathBuf,
}

impl SimplexFactory {
    /// The command line that starts the helper for `data_dir`: `(program, args)`.
    pub fn command(&self, data_dir: &Path) -> (PathBuf, Vec<OsString>) {
        let Some(bwrap) = &self.bwrap else {
            return (self.helper.clone(), Vec::new());
        };
        let mut a: Vec<OsString> = [
            "--unshare-all",
            "--share-net",
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
        let mut ro = |p: &Path, try_: bool| {
            a.push(if try_ { "--ro-bind-try" } else { "--ro-bind" }.into());
            a.push(p.into());
            a.push(p.into());
        };
        for p in SYSTEM_BINDS {
            ro(Path::new(p), true);
        }
        ro(&self.helper, false);
        if let Some(dir) = self.library.parent() {
            ro(dir, false);
        }
        a.extend(["--bind".into(), data_dir.into(), data_dir.into()]);
        a.extend(["--chdir".into(), data_dir.into(), "--".into()]);
        a.push(self.helper.clone().into());
        (bwrap.clone(), a)
    }

    fn env(&self, data_dir: &Path) -> Vec<(OsString, OsString)> {
        let mut env: Vec<(OsString, OsString)> = vec![
            ("HOME".into(), data_dir.into()),
            ("LANG".into(), "C.UTF-8".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ];
        if let Some(p) = &self.library_path {
            env.push(("LD_LIBRARY_PATH".into(), p.clone()));
        }
        env
    }
}

impl EngineFactory for SimplexFactory {
    fn available(&self) -> bool {
        self.helper.is_file() && self.library.is_file()
    }

    fn features(&self, relays: &[String]) -> crate::model::ChatFeatures {
        protocol::features_with(relays)
    }

    fn start(&self, config: EngineConfig, sink: EventSink) -> Result<Box<dyn Engine>> {
        if !self.available() {
            return Err(Error::new(ErrorCode::ChtUnavailable));
        }
        if self.bwrap.is_none() {
            tracing::warn!("chat: bwrap not found; the chat engine runs without a sandbox");
        }
        let (program, args) = self.command(&config.dir);
        let env = self.env(&config.dir);
        let mut child = spawn_piped_helper(&PipedHelper {
            program: &program,
            args: &args,
            env: &env,
            log: &self.log,
        })
        .map_err(|e| Error::new(ErrorCode::ChtUnavailable).with_source(e))?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(Error::new(ErrorCode::ChtUnavailable));
        };
        let pid = child.id();

        let (tx, rx) = channel::<String>();
        let first = serde_json::json!({
            "v": 1,
            "lib": self.library,
            "db": config.dir.join(DB_PREFIX),
            "key": config.key,
        });
        let _ = tx.send(first.to_string());
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(f)
                .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))
        };
        spawn(
            "chat-write",
            Box::new(move || {
                for line in rx {
                    if writeln!(stdin, "{line}")
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
                // Dropping stdin closes it: the helper closes its database and exits.
            }),
        )?;

        // The codec's time (how long since the last relay check).
        let clock = crate::clock::SystemClock::new();
        let codec = Arc::new(Mutex::new(Codec::new(
            &config.name,
            config.relays,
            &config.dir,
        )));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let codec = Arc::clone(&codec);
            let tx = tx.clone();
            let sink = Arc::clone(&sink);
            let exited = Arc::clone(&exited);
            spawn(
                "chat-read",
                Box::new(move || {
                    for line in BufReader::new(stdout).lines() {
                        let Ok(line) = line else { break };
                        let out = {
                            let mut c = lock(&codec);
                            c.tick(clock.mono());
                            c.on_line(&line)
                        };
                        for l in out.lines {
                            let _ = tx.send(l);
                        }
                        for e in out.events {
                            sink(e);
                        }
                    }
                    drop(tx);
                    let status = child.wait().ok().and_then(|s| s.code());
                    exited.store(true, Ordering::SeqCst);
                    let started = lock(&codec).started();
                    sink(ChatEvent::Exited { status, started });
                }),
            )?;
        }
        Ok(Box::new(SimplexEngine {
            tx: Some(tx),
            codec,
            sink,
            pid,
            exited,
            clock,
        }))
    }
}

fn lock(codec: &Mutex<Codec>) -> std::sync::MutexGuard<'_, Codec> {
    codec
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A running helper.
struct SimplexEngine {
    tx: Option<Sender<String>>,
    codec: Arc<Mutex<Codec>>,
    sink: EventSink,
    pid: u32,
    exited: Arc<AtomicBool>,
    clock: crate::clock::SystemClock,
}

impl Engine for SimplexEngine {
    fn shutdown(&mut self) {
        self.tx = None;
        let step = Duration::from_millis(50);
        let mut waited = Duration::ZERO;
        while !self.exited.load(Ordering::SeqCst) && waited < STOP_GRACE {
            std::thread::sleep(step);
            waited += step;
        }
    }

    fn request(&self, request: Request) {
        let out = {
            let mut c = lock(&self.codec);
            c.tick(self.clock.mono());
            c.request(request)
        };
        if let Some(tx) = &self.tx {
            for l in out.lines {
                let _ = tx.send(l);
            }
        }
        for e in out.events {
            (self.sink)(e);
        }
    }
}

impl Drop for SimplexEngine {
    fn drop(&mut self) {
        // Closing the channel ends the writer, which closes the helper's input.
        self.tx = None;
        let exited = Arc::clone(&self.exited);
        let pid = self.pid;
        let _ = std::thread::Builder::new()
            .name("chat-stop".into())
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn factory(bwrap: Option<&str>) -> SimplexFactory {
        SimplexFactory {
            helper: "/opt/easyplay/libexec/easyplay-chat".into(),
            library: "/opt/simplex/lib/libsimplex.so".into(),
            bwrap: bwrap.map(PathBuf::from),
            library_path: None,
            log: "/tmp/x.log".into(),
        }
    }

    #[test]
    fn the_helper_sees_only_its_data_folder_writable_and_never_the_home_folder() {
        let (program, args) = factory(Some("/usr/bin/bwrap"))
            .command(Path::new("/home/u/.local/share/easyplay/chat"));
        assert_eq!(program, PathBuf::from("/usr/bin/bwrap"));
        let args: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let writable: Vec<&String> = args
            .windows(2)
            .filter(|w| w[0] == "--bind")
            .map(|w| &w[1])
            .collect();
        assert_eq!(writable, vec!["/home/u/.local/share/easyplay/chat"]);
        assert!(!args.iter().any(|a| a == "/home" || a == "/home/u"));
        for flag in [
            "--unshare-all",
            "--share-net",
            "--die-with-parent",
            "--new-session",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        assert!(
            args.windows(3)
                .any(|w| w[0] == "--ro-bind" && w[1] == "/opt/simplex/lib")
        );
        assert_eq!(args.last().unwrap(), "/opt/easyplay/libexec/easyplay-chat");
    }

    #[test]
    fn without_bwrap_the_helper_runs_directly() {
        let (program, args) = factory(None).command(Path::new("/d"));
        assert_eq!(
            program,
            PathBuf::from("/opt/easyplay/libexec/easyplay-chat")
        );
        assert!(args.is_empty());
    }

    #[test]
    fn a_missing_helper_is_unavailable() {
        let f = factory(None);
        assert!(!f.available());
        let sink: EventSink = Arc::new(|_| {});
        let cfg = EngineConfig {
            dir: "/nonexistent".into(),
            key: "k".into(),
            name: "n".into(),
            relays: Vec::new(),
        };
        let err = f.start(cfg, sink).err().unwrap();
        assert_eq!(err.code, ErrorCode::ChtUnavailable);
    }
}
