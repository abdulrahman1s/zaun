//! The new version's trial (ADR-0040 §5): GUI and service starts are counted until one
//! confirms; the third start without a confirmation puts the previous version back.

use super::UpdateError;
use super::apply::{Trial, remove_if_present, rollback, write_trial};
use super::install::Layout;
use super::stage::clear_dir;
use super::version::Version;
use crate::fsx::Fs;

/// Starts without a confirmation that are tolerated before rolling back.
pub const MAX_UNCONFIRMED_STARTS: u32 = 2;

/// What a start of the GUI or the service found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrialStart {
    /// No trial for this version.
    None,
    /// Counted; confirm once the window or the service runs.
    Counting,
    /// The version failed too often: the previous one is back (restart into it).
    RolledBack { failed: String },
}

fn read(fs: &dyn Fs, layout: &Layout) -> Option<Trial> {
    let b = fs.read(&layout.trial_file(), 4096).ok()?;
    serde_json::from_slice(&b).ok()
}

/// Count a start of `running`.
pub fn on_start(
    fs: &dyn Fs,
    layout: &Layout,
    running: &Version,
) -> Result<TrialStart, UpdateError> {
    let Some(mut t) = read(fs, layout) else {
        return Ok(TrialStart::None);
    };
    if Version::parse(&t.version).as_ref() != Some(running) {
        // Another version runs (rolled back, or reinstalled by hand): the trial is over.
        remove_if_present(fs, &layout.trial_file())?;
        return Ok(TrialStart::None);
    }
    if t.starts >= MAX_UNCONFIRMED_STARTS {
        rollback(fs, layout)?;
        return Ok(TrialStart::RolledBack { failed: t.version });
    }
    t.starts += 1;
    write_trial(fs, layout, &t)?;
    Ok(TrialStart::Counting)
}

/// The window was drawn (or the service runs): the version works; drop what was kept.
pub fn confirm(fs: &dyn Fs, layout: &Layout, running: &Version) -> Result<bool, UpdateError> {
    let Some(t) = read(fs, layout) else {
        return Ok(false);
    };
    if Version::parse(&t.version).as_ref() != Some(running) {
        return Ok(false);
    }
    clear_dir(fs, &layout.previous())?;
    remove_if_present(fs, &layout.trial_file())?;
    Ok(true)
}
