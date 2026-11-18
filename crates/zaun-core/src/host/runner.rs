//! `CommandRunner`: the seam through which probes run their fixed commands.
//!
//! Every probe command is a fixed argument vector (no shell), runs with a constructed
//! environment (nothing inherited), with standard input from `/dev/null`, a timeout and a cap
//! on captured output. [`RealRunner`] is the implementation; tests use
//! `zaun_testkit::ScriptedRunner`. Together with `session::spawn` this is the only place
//! in `zaun-core` that starts processes (ARCH-002).

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::clock::Clock;

/// Default timeout of a probe command (BUBBLEWRAP_SANDBOX §10: 5 s).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Captured bytes per stream; the rest is discarded.
pub const OUTPUT_CAP: usize = 256 * 1024;

/// One command to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// Absolute path of the program.
    pub program: PathBuf,
    /// Arguments after the program.
    pub args: Vec<OsString>,
    /// The complete environment of the child (nothing is inherited).
    pub env: Vec<(OsString, OsString)>,
    /// Wall-clock budget; the child is killed when it is exceeded.
    pub timeout: Duration,
}

impl CommandSpec {
    /// `program args…` with a minimal environment (`PATH=/usr/bin:/bin`, `LANG=C.UTF-8`)
    /// and [`PROBE_TIMEOUT`].
    pub fn new<S: AsRef<OsStr>>(program: impl AsRef<Path>, args: &[S]) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: args.iter().map(|a| a.as_ref().to_owned()).collect(),
            env: vec![
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("LANG".into(), "C.UTF-8".into()),
            ],
            timeout: PROBE_TIMEOUT,
        }
    }

    /// Builder: add an environment variable.
    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let key = key.into();
        self.env.retain(|(k, _)| *k != key);
        self.env.push((key, value.into()));
        self
    }

    /// Builder: set the timeout.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Program followed by the arguments (what fakes match on).
    pub fn argv(&self) -> Vec<OsString> {
        std::iter::once(self.program.clone().into_os_string())
            .chain(self.args.iter().cloned())
            .collect()
    }

    /// The command line for diagnostics (lossy, space-separated; never executed).
    pub fn display(&self) -> String {
        self.argv()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What a finished (or killed) command produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit code; `128 + n` when killed by signal `n` (shell convention).
    pub status: i32,
    /// Standard output, at most [`OUTPUT_CAP`] bytes.
    pub stdout: Vec<u8>,
    /// Standard error, at most [`OUTPUT_CAP`] bytes.
    pub stderr: Vec<u8>,
    /// The timeout was reached and the child was killed.
    pub timed_out: bool,
}

impl CommandOutput {
    /// Exited with status 0 in time.
    pub fn success(&self) -> bool {
        self.status == 0 && !self.timed_out
    }

    /// Standard output as text (lossy).
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Standard error as text (lossy).
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Why a command could not be run at all.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The program does not exist or is not executable.
    #[error("cannot start {program}: {source}")]
    Spawn {
        /// The program.
        program: PathBuf,
        /// The OS error.
        source: std::io::Error,
    },
    /// Waiting for or reading from the child failed.
    #[error("waiting for {program}: {source}")]
    Wait {
        /// The program.
        program: PathBuf,
        /// The OS error.
        source: std::io::Error,
    },
    /// A test fake had no expectation for the command.
    #[error("unexpected command: {0}")]
    Unexpected(String),
}

/// Runs probe commands.
pub trait CommandRunner: Send + Sync {
    /// Run `cmd` to completion or until its timeout.
    fn run(&self, cmd: &CommandSpec) -> Result<CommandOutput, RunError>;
}

impl<R: CommandRunner + ?Sized> CommandRunner for Arc<R> {
    fn run(&self, cmd: &CommandSpec) -> Result<CommandOutput, RunError> {
        (**self).run(cmd)
    }
}

impl<R: CommandRunner + ?Sized> CommandRunner for &R {
    fn run(&self, cmd: &CommandSpec) -> Result<CommandOutput, RunError> {
        (**self).run(cmd)
    }
}

/// The real runner: `std::process` with a polling timeout measured on the monotonic clock.
pub struct RealRunner {
    clock: Arc<dyn Clock>,
}

impl RealRunner {
    /// A runner measuring timeouts with `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { clock }
    }
}

impl std::fmt::Debug for RealRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RealRunner")
    }
}

fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = OUTPUT_CAP.saturating_sub(out.len());
                    out.extend_from_slice(&buf[..n.min(room)]);
                }
            }
        }
        out
    })
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(c), _) => c,
        (None, Some(sig)) => 128 + sig,
        (None, None) => -1,
    }
}

impl RealRunner {
    fn wait(&self, child: &mut Child, cmd: &CommandSpec) -> Result<(i32, bool), RunError> {
        let deadline = self.clock.mono().saturating_add(cmd.timeout);
        let mut pause = Duration::from_millis(2);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok((exit_code(status), false)),
                Ok(None) => {}
                Err(source) => {
                    return Err(RunError::Wait {
                        program: cmd.program.clone(),
                        source,
                    });
                }
            }
            if self.clock.mono() >= deadline {
                let _ = child.kill();
                let status = child.wait().map_err(|source| RunError::Wait {
                    program: cmd.program.clone(),
                    source,
                })?;
                return Ok((exit_code(status), true));
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_millis(50));
        }
    }
}

impl CommandRunner for RealRunner {
    #[allow(clippy::disallowed_methods)] // host::runner is one of the two places that spawn.
    fn run(&self, cmd: &CommandSpec) -> Result<CommandOutput, RunError> {
        crate::ui_thread::assert_not_ui_thread("CommandRunner::run");
        let mut command = std::process::Command::new(&cmd.program);
        command
            .args(&cmd.args)
            .env_clear()
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|source| RunError::Spawn {
            program: cmd.program.clone(),
            source,
        })?;
        let out = child.stdout.take().map(drain);
        let err = child.stderr.take().map(drain);
        let (status, timed_out) = self.wait(&mut child, cmd)?;
        // A grandchild that keeps the pipes open would block the readers; after a timeout
        // the streams are abandoned rather than joined.
        let join = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
            h.filter(|h| !timed_out || h.is_finished())
                .and_then(|h| h.join().ok())
                .unwrap_or_default()
        };
        Ok(CommandOutput {
            status,
            stdout: join(out),
            stderr: join(err),
            timed_out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;

    fn runner() -> RealRunner {
        RealRunner::new(Arc::new(SystemClock::new()))
    }

    fn sh() -> Option<PathBuf> {
        ["/bin/sh", "/usr/bin/sh", "/run/current-system/sw/bin/sh"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
    }

    #[test]
    fn runs_with_clean_environment() {
        let Some(sh) = sh() else { return };
        let out = runner()
            .run(
                &CommandSpec::new(&sh, &["-c", "echo \"$FOO:$HOME\"; echo err >&2; exit 3"])
                    .env("FOO", "bar"),
            )
            .unwrap();
        assert_eq!(out.status, 3);
        assert_eq!(out.stdout_text(), "bar:\n");
        assert_eq!(out.stderr_text(), "err\n");
        assert!(!out.timed_out && !out.success());
    }

    #[test]
    fn kills_on_timeout() {
        let Some(sh) = sh() else { return };
        let out = runner()
            .run(
                &CommandSpec::new(&sh, &["-c", "while :; do :; done"])
                    .timeout(Duration::from_millis(100)),
            )
            .unwrap();
        assert!(out.timed_out);
        assert_eq!(out.status, 128 + 9);
    }

    #[test]
    fn missing_program() {
        assert!(matches!(
            runner().run(&CommandSpec::new("/nonexistent/prog", &["x"])),
            Err(RunError::Spawn { .. })
        ));
    }

    #[test]
    fn argv_and_env_builder() {
        let c = CommandSpec::new("/usr/bin/bwrap", &["--version"]).env("PATH", "/x");
        assert_eq!(
            c.argv(),
            vec![OsString::from("/usr/bin/bwrap"), "--version".into()]
        );
        assert_eq!(c.env.iter().filter(|(k, _)| k == "PATH").count(), 1);
        assert_eq!(c.display(), "/usr/bin/bwrap --version");
    }
}
