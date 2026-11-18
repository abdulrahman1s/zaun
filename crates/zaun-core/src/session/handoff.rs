//! Handing a request to a program Zaun does not own (ADR-0031 §5): the Steam client.
//!
//! [`RealHandoff`] starts the command detached exactly like the supervisor
//! (`sys::configure_detached`: `setsid`, double fork, every other descriptor closed), with
//! standard streams on `/dev/null` and the command's environment only. It waits for the
//! intermediate process alone; the program itself is never waited for, signalled or tracked.
//! An `exec` failure of the program is reported by the spawn (close-on-exec error pipe).
//!
//! This is the third place in `zaun-core` that starts processes, next to
//! `host::runner` (probes, which it waits for) and `session::spawn` / `session::launcher`
//! (sandboxes and their supervisors) — ARCH-002.

use std::process::{Command, Stdio};

use super::sys;
use crate::error::{Error, ErrorCode};
use crate::host::CommandSpec;
use crate::steam::Handoff;

/// The real hand-off.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealHandoff;

impl Handoff for RealHandoff {
    #[allow(clippy::disallowed_methods)] // session::handoff starts a program it does not own (ARCH-002).
    fn start_detached(&self, cmd: &CommandSpec) -> Result<(), Error> {
        crate::ui_thread::assert_not_ui_thread("Handoff::start_detached");
        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args)
            .env_clear()
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        sys::configure_detached(&mut c);
        let mut intermediate = c.spawn().map_err(|e| {
            Error::new(ErrorCode::StmClientUnavailable)
                .with("program", cmd.program.display())
                .with_source(e)
        })?;
        // The intermediate exits at once; reap it.
        let _ = intermediate.wait();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_reported() {
        let err = RealHandoff
            .start_detached(&CommandSpec::new(
                "/nonexistent/steam",
                &["steam://open/downloads"],
            ))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::StmClientUnavailable);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // A real child process against wall time.
    fn a_program_is_started_and_not_waited_for() {
        let Some(sh) = ["/bin/sh", "/usr/bin/sh", "/run/current-system/sw/bin/sh"]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.exists())
        else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let script = format!("sleep 0.2; : > '{}'", marker.display());
        let started = std::time::Instant::now();
        RealHandoff
            .start_detached(&CommandSpec::new(&sh, &["-c", script.as_str()]))
            .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_millis(150),
            "not waited for"
        );
        for _ in 0..100 {
            if marker.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the detached program did not run");
    }
}
