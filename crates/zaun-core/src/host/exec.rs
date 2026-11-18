//! Locating executables through [`Fs`] (shared by the bwrap and umu probes).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::fsx::Fs;

/// Whether `path` (following symlinks) is a regular file with an execute bit set.
pub fn is_executable(fs: &dyn Fs, path: &Path) -> bool {
    fs.metadata_follow(path)
        .is_ok_and(|md| md.is_file() && md.mode & 0o111 != 0)
}

/// Absolute directories of a `PATH` value, in order; relative and empty entries are skipped.
pub fn path_dirs(path: Option<&OsStr>) -> Vec<PathBuf> {
    path.map(|p| {
        std::env::split_paths(p)
            .filter(|d| d.is_absolute())
            .collect()
    })
    .unwrap_or_default()
}

/// The first `dir/name` in `dirs` that is executable.
pub fn find_in(fs: &dyn Fs, dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|d| d.join(name))
        .find(|p| is_executable(fs, p))
}
