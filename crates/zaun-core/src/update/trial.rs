//! Which program the installed copy starts, and the new version's trial (ADR-0041 §4).
//!
//! The installed copy is the launcher: it starts `current` when that is newer than itself,
//! and counts the GUI and service starts of a version on trial before starting it, so a
//! version that cannot even load is rolled back too. The version confirms itself once its
//! window is drawn or its service runs; the third start without that puts the previous
//! version (or the installed copy) back.

use std::path::PathBuf;

use super::UpdateError;
use super::apply::{read_trial, rollback, write_trial};
use super::store::{Store, remove_if_present};
use super::version::Version;
use crate::fsx::Fs;

/// Starts without a confirmation that are tolerated before rolling back.
pub const MAX_UNCONFIRMED_STARTS: u32 = 2;

/// What the installed copy does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    /// The program to start instead of itself; `None` = run itself.
    pub start: Option<PathBuf>,
    /// A version that failed its trial and was rolled back just now.
    pub rolled_back: Option<Version>,
}

/// Choose for the installed copy of version `installed`. `counted` for a GUI or service
/// start (CLI verbs neither count nor confirm).
pub fn choose(
    fs: &dyn Fs,
    store: &Store,
    installed: &Version,
    counted: bool,
) -> Result<Choice, UpdateError> {
    let mut choice = Choice::default();
    let Some((mut v, mut dir)) = store.linked(fs, &store.current()) else {
        return Ok(choice);
    };
    if counted && let Some(mut t) = read_trial(fs, store) {
        if Version::parse(&t.version).as_ref() != Some(&v) {
            // The trial of a version that is no longer current.
            remove_if_present(fs, &store.trial_file())?;
        } else if t.starts >= MAX_UNCONFIRMED_STARTS {
            choice.rolled_back = rollback(fs, store)?;
            let Some(back) = store.linked(fs, &store.current()) else {
                return Ok(choice);
            };
            (v, dir) = back;
        } else {
            t.starts += 1;
            write_trial(fs, store, &t)?;
        }
    }
    // A package manager may have installed something at least as new meanwhile.
    if v <= *installed {
        return Ok(choice);
    }
    let bin = Store::binary(&dir);
    if fs
        .metadata_follow(&bin)
        .is_ok_and(|m| m.is_file() && m.mode & 0o111 != 0)
    {
        choice.start = Some(bin);
    }
    Ok(choice)
}

/// The window was drawn (or the service runs): `running` works. Ends its trial and removes
/// every other version and unused shared folder.
pub fn confirm(fs: &dyn Fs, store: &Store, running: &Version) -> Result<bool, UpdateError> {
    let Some(t) = read_trial(fs, store) else {
        return Ok(false);
    };
    if Version::parse(&t.version).as_ref() != Some(running) {
        return Ok(false);
    }
    remove_if_present(fs, &store.previous())?;
    remove_if_present(fs, &store.trial_file())?;
    let mut keep = vec![running.clone()];
    if let Some((staged, _)) = store.linked(fs, &store.staged()) {
        keep.push(staged);
    }
    store.prune(fs, &keep);
    Ok(true)
}
