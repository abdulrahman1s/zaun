//! The few reads the UI does itself, on its own worker threads (never the UI thread): a file
//! the user picked or dropped, and whether a program is on `PATH`. The UI may not touch
//! `fsx` or `host::exec` (ARCHITECTURE §3 rule 2); these are its doors to them.
use std::path::Path;

use crate::fsx::{Fs, RealFs};
use crate::host::exec::{find_in, path_dirs};

/// A file the user chose, at most `limit` bytes; the error is for the log.
pub fn read_picked_file(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    RealFs::new().read(path, limit).map_err(|e| e.to_string())
}

/// Whether `program` is found on this process's `PATH`.
pub fn on_path(program: &str) -> bool {
    let dirs = path_dirs(std::env::var_os("PATH").as_deref());
    find_in(&RealFs::new(), &dirs, program).is_some()
}
