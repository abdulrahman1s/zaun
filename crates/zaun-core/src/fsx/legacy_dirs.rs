//! One-time move of the pre-rename `easyplay` XDG directories to `zaun` (ADR-0039).

use std::path::{Path, PathBuf};

use super::Fs;
use crate::consts::{DIR_NAME, ENV_CACHE_DIR, ENV_CONFIG_DIR, ENV_DATA_DIR, ENV_STATE_DIR};
use crate::paths::{AppPaths, PathEnv};

/// The directory name before the app was renamed to Zaun.
pub const LEGACY_DIR_NAME: &str = "easyplay";

/// What happened to one old directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyMove {
    /// Renamed to the new name; a link at the old path points to it.
    Moved { from: PathBuf, to: PathBuf },
    /// Renamed, but the link at the old path could not be made.
    LinkFailed {
        from: PathBuf,
        to: PathBuf,
        error: String,
    },
    /// The old path was an empty directory next to the new one (a mount point left behind
    /// when the data was moved outside the app, e.g. by an impermanence setup): it is now a
    /// link to the new one.
    Linked { from: PathBuf, to: PathBuf },
    /// Both exist and the old one is not empty: nothing was touched.
    BothExist { from: PathBuf, to: PathBuf },
    /// The rename failed: nothing was touched.
    Failed {
        from: PathBuf,
        to: PathBuf,
        error: String,
    },
}

/// Move `<root>/easyplay` to `<root>/zaun` for config, data, state and cache, and leave a
/// symlink at the old path so absolute paths stored by the old version keep resolving. Never
/// deletes and never overwrites. A directory with a `ZAUN_*_DIR` override is left alone.
/// Run before anything opens a directory; running it again does nothing.
pub fn adopt_legacy_dirs(fs: &dyn Fs, env: &PathEnv, paths: &AppPaths) -> Vec<LegacyMove> {
    [
        (ENV_CONFIG_DIR, &paths.config_dir),
        (ENV_DATA_DIR, &paths.data_dir),
        (ENV_STATE_DIR, &paths.state_dir),
        (ENV_CACHE_DIR, &paths.cache_dir),
    ]
    .into_iter()
    .filter(|(var, _)| env.get(var).is_none_or(|v| v.is_empty()))
    .filter_map(|(_, new)| move_one(fs, new))
    .collect()
}

/// Both exist: an empty old directory becomes a link to the new one; one with anything in it
/// is left alone (removing only an empty directory never loses data).
fn link_leftover(fs: &dyn Fs, from: PathBuf, to: PathBuf) -> LegacyMove {
    let empty = fs.read_dir(&from).is_ok_and(|e| e.is_empty());
    if !empty || fs.remove_dir(&from).is_err() {
        return LegacyMove::BothExist { from, to };
    }
    match fs.symlink(&to, &from) {
        Ok(()) => LegacyMove::Linked { from, to },
        Err(e) => LegacyMove::LinkFailed {
            from,
            to,
            error: e.to_string(),
        },
    }
}

fn move_one(fs: &dyn Fs, new: &Path) -> Option<LegacyMove> {
    if new.file_name()? != DIR_NAME {
        return None;
    }
    let from = new.with_file_name(LEGACY_DIR_NAME);
    // A link here is the result of an earlier move.
    if !fs.metadata(&from).ok()?.is_dir() {
        return None;
    }
    let to = new.to_owned();
    if fs.exists(&to) {
        return Some(link_leftover(fs, from, to));
    }
    if let Err(e) = fs.rename(&from, &to) {
        // Another process of ours won the race.
        if fs.exists(&to) {
            return None;
        }
        return Some(LegacyMove::Failed {
            from,
            to,
            error: e.to_string(),
        });
    }
    Some(match fs.symlink(&to, &from) {
        Ok(()) => LegacyMove::Moved { from, to },
        Err(e) => LegacyMove::LinkFailed {
            from,
            to,
            error: e.to_string(),
        },
    })
}
