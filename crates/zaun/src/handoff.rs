//! The installed copy starts the newest version of the bundle store instead of itself
//! (ADR-0041 §4), the way Steam's and Discord's launchers start their updated client. Runs
//! first in `main`, before anything opens a file of Zaun's.

use std::ffi::OsString;
use std::path::PathBuf;

use zaun_core::fsx::{Fs, RealFs};
use zaun_core::host::replace_process_with;
use zaun_core::logging::ProcessRole;
use zaun_core::model::InstallChannel;
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::update::state::state_file;
use zaun_core::update::{UpdateState, Version, trial};

/// Set for the started version: the installed copy that started it, which a restart after
/// an update goes through again.
const INSTALLED_COPY: &str = "ZAUN_INSTALLED_COPY";
/// Set to anything to always run the installed copy.
const OFF: &str = "ZAUN_NO_HANDOFF";

/// The installed copy that started this version, if it is still there.
pub fn installed_copy() -> Option<PathBuf> {
    std::env::var_os(INSTALLED_COPY)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// Start the newest version instead of this copy when there is one; returns when this copy
/// runs. `role` is `None` for a command line this copy cannot parse. A GUI or service start
/// counts a new version's trial; a session supervisor is always the program that started it.
#[allow(clippy::print_stderr)] // runs before logging is set up
pub fn run(env: &PathEnv, role: Option<ProcessRole>) {
    if role == Some(ProcessRole::Supervisor) || env.get(OFF).is_some_and(|v| !v.is_empty()) {
        return;
    }
    let counted = role == Some(ProcessRole::Gui);
    let Ok(paths) = AppPaths::resolve(env) else {
        return;
    };
    let fs = RealFs::new();
    let install = crate::wiring::installed(&fs, &paths);
    if install.channel == InstallChannel::Bundle {
        return;
    }
    let (Some(store), Some(installed)) = (
        install.store(),
        Version::parse(zaun_core::consts::APP_VERSION),
    ) else {
        return;
    };
    let choice = match trial::choose(&fs, store, &installed, counted) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("zaun: the update store could not be read ({e}); running this copy");
            return;
        }
    };
    if let Some(v) = &choice.rolled_back {
        eprintln!("zaun: version {v} did not start; it was rolled back");
        remember_rollback(&fs, &paths, v);
    }
    let (Some(program), Ok(me)) = (choice.start, std::env::current_exe()) else {
        return;
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let e = replace_process_with(&program, &args, &[(INSTALLED_COPY, me.as_os_str())], &[]);
    // No program interpreter (NixOS without nix-ld), a `noexec` home, …
    eprintln!(
        "zaun: could not start {} ({e}); running the installed copy",
        program.display()
    );
}

/// Settings › About says once that the version was put back.
fn remember_rollback(fs: &dyn Fs, paths: &AppPaths, v: &Version) {
    let path = state_file(paths);
    let mut st = UpdateState::load(fs, &path);
    st.rolled_back = Some(v.to_string());
    // Best effort: the rollback itself is done.
    let _ = st.save(fs, &path);
}
