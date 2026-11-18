//! `ScriptedRunner`: expected argv → canned output. Implements
//! `zaun_core::host::CommandRunner` by matching the command's program followed by its
//! arguments (the environment and timeout are recorded but not matched).

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};

use parking_lot::Mutex;
use zaun_core::host::{CommandOutput, CommandRunner, CommandSpec, RunError};

/// The result of one scripted command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CannedOutput {
    /// Exit status (`-1` for "killed by a signal" in tests that need it).
    pub status: i32,
    /// Standard output.
    pub stdout: Vec<u8>,
    /// Standard error.
    pub stderr: Vec<u8>,
    /// The command "ran" into its timeout.
    pub timed_out: bool,
}

impl CannedOutput {
    /// Exit 0 with `stdout`.
    pub fn ok(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 0,
            stdout: stdout.into(),
            stderr: Vec::new(),
            timed_out: false,
        }
    }

    /// A failure with `status` and `stderr`.
    pub fn fail(status: i32, stderr: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            stdout: Vec::new(),
            stderr: stderr.into(),
            timed_out: false,
        }
    }

    /// Killed after its timeout (status 137).
    pub fn timeout() -> Self {
        Self {
            status: 128 + 9,
            stdout: Vec::new(),
            stderr: Vec::new(),
            timed_out: true,
        }
    }
}

/// A command that no expectation matched.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unexpected command: {0:?}")]
pub struct UnexpectedCommand(pub Vec<OsString>);

#[derive(Debug)]
struct Expectation {
    argv: Vec<OsString>,
    output: CannedOutput,
    /// `None` = any number of times.
    remaining: Option<usize>,
    used: usize,
}

/// Matches whole argument vectors (program first) against expectations, in the order they
/// were added; the first matching expectation with uses left answers.
#[derive(Debug, Default)]
pub struct ScriptedRunner {
    expectations: Mutex<Vec<Expectation>>,
    calls: Mutex<VecDeque<Vec<OsString>>>,
}

fn to_argv<S: AsRef<OsStr>>(argv: &[S]) -> Vec<OsString> {
    argv.iter().map(|a| a.as_ref().to_owned()).collect()
}

impl ScriptedRunner {
    /// No expectations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer `argv` with `output` every time.
    #[must_use]
    pub fn expect<S: AsRef<OsStr>>(self, argv: &[S], output: CannedOutput) -> Self {
        self.expectations.lock().push(Expectation {
            argv: to_argv(argv),
            output,
            remaining: None,
            used: 0,
        });
        self
    }

    /// Answer `argv` with `output` exactly once.
    #[must_use]
    pub fn expect_once<S: AsRef<OsStr>>(self, argv: &[S], output: CannedOutput) -> Self {
        self.expectations.lock().push(Expectation {
            argv: to_argv(argv),
            output,
            remaining: Some(1),
            used: 0,
        });
        self
    }

    /// "Run" a command.
    pub fn run<S: AsRef<OsStr>>(&self, argv: &[S]) -> Result<CannedOutput, UnexpectedCommand> {
        let argv = to_argv(argv);
        self.calls.lock().push_back(argv.clone());
        let mut exps = self.expectations.lock();
        for e in exps.iter_mut() {
            if e.argv == argv && e.remaining != Some(0) {
                e.used += 1;
                if let Some(n) = &mut e.remaining {
                    *n -= 1;
                }
                return Ok(e.output.clone());
            }
        }
        Err(UnexpectedCommand(argv))
    }

    /// Every command run so far, in order.
    pub fn calls(&self) -> Vec<Vec<OsString>> {
        self.calls.lock().iter().cloned().collect()
    }

    /// Expectations that were never used (for `assert!(r.unused().is_empty())`).
    pub fn unused(&self) -> Vec<Vec<OsString>> {
        self.expectations
            .lock()
            .iter()
            .filter(|e| e.used == 0)
            .map(|e| e.argv.clone())
            .collect()
    }
}

impl CommandRunner for ScriptedRunner {
    fn run(&self, cmd: &CommandSpec) -> Result<CommandOutput, RunError> {
        let out = ScriptedRunner::run(self, &cmd.argv())
            .map_err(|e| RunError::Unexpected(format!("{:?}", e.0)))?;
        Ok(CommandOutput {
            status: out.status,
            stdout: out.stdout,
            stderr: out.stderr,
            timed_out: out.timed_out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_answers() {
        let r = ScriptedRunner::new()
            .expect(
                &["bwrap", "--version"],
                CannedOutput::ok("bubblewrap 0.12.0\n"),
            )
            .expect_once(&["umu-run", "--version"], CannedOutput::fail(1, "boom"))
            .expect(&["never"], CannedOutput::ok(""));
        assert_eq!(
            r.run(&["bwrap", "--version"]).unwrap().stdout,
            b"bubblewrap 0.12.0\n"
        );
        assert_eq!(r.run(&["bwrap", "--version"]).unwrap().status, 0);
        assert_eq!(r.run(&["umu-run", "--version"]).unwrap().status, 1);
        assert!(r.run(&["umu-run", "--version"]).is_err());
        assert!(r.run(&["bwrap"]).is_err());
        assert_eq!(r.calls().len(), 5);
        assert_eq!(r.unused(), vec![vec![OsString::from("never")]]);
    }

    #[test]
    fn implements_command_runner() {
        let r = ScriptedRunner::new()
            .expect(
                &["/usr/bin/bwrap", "--version"],
                CannedOutput::ok("bubblewrap 0.12.0\n"),
            )
            .expect(&["/usr/bin/bwrap", "--help"], CannedOutput::timeout());
        let runner: &dyn CommandRunner = &r;
        let out = runner
            .run(&CommandSpec::new("/usr/bin/bwrap", &["--version"]))
            .unwrap();
        assert!(out.success());
        assert!(
            runner
                .run(&CommandSpec::new("/usr/bin/bwrap", &["--help"]))
                .unwrap()
                .timed_out
        );
        assert!(matches!(
            runner.run(&CommandSpec::new("/usr/bin/bwrap", &["x"])),
            Err(RunError::Unexpected(_))
        ));
    }
}
