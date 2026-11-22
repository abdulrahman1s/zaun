//! Switching to a staged version when the GUI or the resident service starts, confirming the
//! running version, and "Restart now" (ADR-0041 §4). Only a copy that updates itself does any
//! of this; the installed copy's hand-off is in `handoff`.
//!
//! ```text
//! start ── staged version and no open session? ── switch, start the installed copy again
//! window drawn / service ready ── confirm the trial
//! window closed with "Restart now" ── switch, start the installed copy again
//! ```

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use zaun_core::clock::SystemClock;
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::host::{RealRunner, replace_process};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::storage::{Db, SessionRepo};
use zaun_core::update::source::TRUSTED_KEYS;
use zaun_core::update::{Store, Version, apply, trial};

fn running() -> Option<Version> {
    Version::parse(zaun_core::consts::APP_VERSION)
}

fn store(fs: &dyn Fs, paths: &AppPaths) -> Option<Store> {
    crate::wiring::installed(fs, paths).store().cloned()
}

/// Whether the library lists an open session (unknown counts as yes: nothing is switched
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
pub fn at_start(_env: &PathEnv, paths: &AppPaths) {
    let fs = RealFs::new();
    let (Some(store), Some(running)) = (store(&fs, paths), running()) else {
        return;
    };
    // A note from a "Restart now" whose switch never ran is stale now: this start switches.
    let _ = fs.remove_file(&store.restart_file()); // missing is the usual case
    switch_staged(
        &fs,
        paths,
        &store,
        &running,
        std::env::args_os().skip(1).collect(),
    );
}

/// The window was drawn or the service is ready: the running version works.
pub fn confirm(env: &PathEnv) {
    let fs = RealFs::new();
    let Ok(paths) = AppPaths::resolve(env) else {
        return;
    };
    let (Some(store), Some(running)) = (store(&fs, &paths), running()) else {
        return;
    };
    match trial::confirm(&fs, &store, &running) {
        Ok(true) => tracing::info!(version = %running, "update confirmed"),
        Ok(false) => {}
        Err(e) => tracing::warn!(error = %e, "update not confirmed"),
    }
}

/// Whether "Restart now" was pressed (the resident service leaves its loop for it).
pub fn restart_requested(env: &PathEnv) -> bool {
    let fs = RealFs::new();
    let Ok(paths) = AppPaths::resolve(env) else {
        return false;
    };
    store(&fs, &paths).is_some_and(|s| fs.exists(&s.restart_file()))
}

/// After the window closed: if "Restart now" was pressed, switch and start again with
/// `args`. Returns when there is nothing to do or switching failed.
pub fn restart_if_requested(_env: &PathEnv, paths: &AppPaths, args: Vec<OsString>) {
    let fs = RealFs::new();
    let (Some(store), Some(running)) = (store(&fs, paths), running()) else {
        return;
    };
    if fs.remove_file(&store.restart_file()).is_err() {
        return;
    }
    if !switch_staged(&fs, paths, &store, &running, args) {
        tracing::warn!("restart for the update: nothing was switched");
    }
}

/// Switch to a staged version when no session is open, then start again. Returns `false`
/// when nothing was switched.
fn switch_staged(
    fs: &dyn Fs,
    paths: &AppPaths,
    store: &Store,
    running: &Version,
    args: Vec<OsString>,
) -> bool {
    if !fs.exists(&store.staged()) || sessions_open(fs, paths) {
        return false;
    }
    let runner = RealRunner::new(Arc::new(SystemClock::new()));
    match apply::apply(
        fs,
        &runner,
        store,
        TRUSTED_KEYS,
        running,
        std::env::consts::ARCH,
    ) {
        Ok(Some(v)) => {
            tracing::info!(version = %v, "switched to the new version; starting it");
            relaunch(fs, store, args);
            false
        }
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(code = e.code().as_str(), error = %e, "update not switched to");
            false
        }
    }
}

/// Start again through the installed copy, which counts the new version's trial and starts
/// it (returns only if that failed). Without one (a version started by hand), the new
/// version directly.
fn relaunch(fs: &dyn Fs, store: &Store, args: Vec<OsString>) {
    let launcher = crate::handoff::installed_copy().or_else(|| {
        std::env::current_exe()
            .ok()
            .filter(|e| !store.contains(fs, e))
    });
    let target = launcher.or_else(|| {
        store
            .linked(fs, &store.current())
            .map(|(_, dir)| Store::binary(&dir))
    });
    let Some(target): Option<PathBuf> = target else {
        tracing::error!("no program to start after the update");
        return;
    };
    let e = replace_process(&target, &args);
    tracing::error!(error = %e, program = %target.display(), "could not start again");
}
