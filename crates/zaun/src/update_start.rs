//! Installing a staged update when the GUI or the resident service starts, the new version's
//! trial, and "Restart now" (ADR-0040 §5). Only an owned tarball install does any of this.
//!
//! ```text
//! start ── trial: 3rd unconfirmed start? ── roll back, replace the process
//!       └─ staged update and no open session? ── apply, replace the process
//! window drawn / service ready ── confirm the trial
//! window closed with "Restart now" ── apply, replace the process
//! ```

use std::ffi::OsString;
use std::sync::Arc;

use zaun_core::clock::SystemClock;
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::host::{RealRunner, replace_process};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::storage::{Db, SessionRepo};
use zaun_core::update::source::TRUSTED_KEYS;
use zaun_core::update::trial::{self, TrialStart};
use zaun_core::update::{Layout, UpdateState, Version, apply, state};

fn running() -> Option<Version> {
    Version::parse(zaun_core::consts::APP_VERSION)
}

fn layout(fs: &dyn Fs, env: &PathEnv) -> Option<Layout> {
    crate::wiring::installed(fs, env.uid()).layout()
}

/// Whether the library lists an open session (unknown counts as yes: nothing is replaced
/// until the service has run its recovery).
fn sessions_open(fs: &dyn Fs, paths: &AppPaths) -> bool {
    if !fs.exists(&paths.db_file) {
        return false;
    }
    Db::open_ro(&paths.db_file)
        .ok()
        .and_then(|mut db| db.open_sessions().ok())
        .is_none_or(|s| !s.is_empty())
}

/// Before the GUI or the service opens the library. May replace the process.
pub fn at_start(env: &PathEnv, paths: &AppPaths) {
    let fs = RealFs::new();
    let (Some(layout), Some(running)) = (layout(&fs, env), running()) else {
        return;
    };
    match trial::on_start(&fs, &layout, &running) {
        Ok(TrialStart::RolledBack { failed }) => {
            tracing::warn!(version = %failed, "the new version did not start; the previous one is back");
            let path = state::state_file(paths);
            let mut st = UpdateState::load(&fs, &path);
            st.rolled_back = Some(failed);
            if let Err(e) = st.save(&fs, &path) {
                tracing::warn!(error = %e, "update state not saved");
            }
            restart(&layout, std::env::args_os().skip(1).collect());
        }
        Ok(TrialStart::Counting | TrialStart::None) => {}
        Err(e) => tracing::warn!(error = %e, "update trial not read"),
    }
    // A note from a "Restart now" whose apply never ran is stale now: this start applies.
    let _ = fs.remove_file(&layout.restart_file()); // missing is the usual case
    install_staged(
        &fs,
        paths,
        &layout,
        &running,
        std::env::args_os().skip(1).collect(),
    );
}

/// The window was drawn or the service is ready: the running version works.
pub fn confirm(env: &PathEnv) {
    let fs = RealFs::new();
    let (Some(layout), Some(running)) = (layout(&fs, env), running()) else {
        return;
    };
    match trial::confirm(&fs, &layout, &running) {
        Ok(true) => tracing::info!(version = %running, "update confirmed"),
        Ok(false) => {}
        Err(e) => tracing::warn!(error = %e, "update not confirmed"),
    }
}

/// Whether "Restart now" was pressed (the resident service leaves its loop for it).
pub fn restart_requested(env: &PathEnv) -> bool {
    let fs = RealFs::new();
    layout(&fs, env).is_some_and(|l| fs.exists(&l.restart_file()))
}

/// After the window closed: if "Restart now" was pressed, install and start again with
/// `args`. Returns when there is nothing to do or installing failed.
pub fn restart_if_requested(env: &PathEnv, paths: &AppPaths, args: Vec<OsString>) {
    let fs = RealFs::new();
    let (Some(layout), Some(running)) = (layout(&fs, env), running()) else {
        return;
    };
    if fs.remove_file(&layout.restart_file()).is_err() {
        return;
    }
    if !install_staged(&fs, paths, &layout, &running, args) {
        tracing::warn!("restart for the update: nothing was installed");
    }
}

/// Apply a staged update when no session is open, then replace the process. Returns `false`
/// when nothing was installed.
fn install_staged(
    fs: &dyn Fs,
    paths: &AppPaths,
    layout: &Layout,
    running: &Version,
    args: Vec<OsString>,
) -> bool {
    if !fs.exists(&layout.staged()) || sessions_open(fs, paths) {
        return false;
    }
    let runner = RealRunner::new(Arc::new(SystemClock::new()));
    match apply::apply(
        fs,
        &runner,
        layout,
        TRUSTED_KEYS,
        running,
        std::env::consts::ARCH,
    ) {
        Ok(Some(v)) => {
            tracing::info!(version = %v, "update installed; starting it");
            restart(layout, args);
            false
        }
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(code = e.code().as_str(), error = %e, "update not installed");
            false
        }
    }
}

/// Become the installed binary (returns only if that failed).
fn restart(layout: &Layout, args: Vec<OsString>) {
    let e = replace_process(&layout.binary(), &args);
    tracing::error!(error = %e, "could not start the installed binary");
}
