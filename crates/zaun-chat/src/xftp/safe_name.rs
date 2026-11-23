//! A file name from a stranger's header, made safe to create in a folder of ours.
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::error::XftpError;

const MAX_NAME_BYTES: usize = 200;

/// The last path component without control characters, never empty, `.` or `..`.
#[must_use]
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut clean: String = base.chars().filter(|c| !c.is_control()).collect();
    clean = clean.trim_matches([' ', '.']).to_owned();
    while clean.len() > MAX_NAME_BYTES {
        clean.pop();
    }
    if clean.is_empty() {
        "file".to_owned()
    } else {
        clean
    }
}

/// Claims a new empty file `name`, `name (1)`, `name (2)` … in `dir` and returns its path.
pub fn claim_unique(dir: &Path, name: &str) -> Result<PathBuf, XftpError> {
    let (stem, ext) = match name.rfind('.') {
        Some(at) if at > 0 => (&name[..at], &name[at..]),
        _ => (name, ""),
    };
    for n in 0..1000 {
        let candidate = dir.join(if n == 0 {
            name.to_owned()
        } else {
            format!("{stem} ({n}){ext}")
        });
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(XftpError::File("no free file name".into()))
}
