//! Atomic file replacement: write-temp, `fsync`, rename, `fsync` directory (PERSISTENCE §4).

#![allow(clippy::disallowed_methods)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{FsError, FsErrorKind, FsOp, FsResult};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A point in the write sequence (fault injection in tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    /// After the temporary file was written and synced, before the rename.
    BeforeRename,
}

fn temp_path(path: &Path) -> FsResult<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| FsError::new(FsOp::Write, path, FsErrorKind::InvalidInput))?;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut tmp = std::ffi::OsString::from(".");
    tmp.push(name);
    tmp.push(format!(".tmp.{}.{n}", std::process::id()));
    Ok(path.with_file_name(tmp))
}

/// Replace `path` with `data` atomically, creating it with `mode` if needed.
pub fn atomic_write_real(path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
    atomic_write_with(path, data, mode, &|_| Ok(()))
}

pub(crate) fn atomic_write_with(
    path: &Path,
    data: &[u8],
    mode: u32,
    hook: &dyn Fn(Stage) -> io::Result<()>,
) -> FsResult<()> {
    let tmp = temp_path(path)?;
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(super::real::o_nofollow())
            .open(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        hook(Stage::BeforeRename)?;
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(FsError::from_io(FsOp::Write, path, e));
    }
    if let Some(dir) = path.parent() {
        // Persist the rename. Directories can be opened read-only for fsync on Linux.
        File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| FsError::from_io(FsOp::Write, dir, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn replaces_content_and_sets_mode() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        atomic_write_real(&p, b"one", 0o600).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        atomic_write_real(&p, b"two", 0o600).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failure_leaves_old_content_and_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        atomic_write_real(&p, b"old", 0o600).unwrap();
        let err = atomic_write_with(&p, b"new", 0o600, &|stage| {
            assert_eq!(stage, Stage::BeforeRename);
            Err(io::Error::from_raw_os_error(28)) // ENOSPC
        })
        .unwrap_err();
        assert_eq!(err.kind, FsErrorKind::NoSpace);
        assert_eq!(fs::read(&p).unwrap(), b"old");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope/file");
        assert_eq!(
            atomic_write_real(&p, b"x", 0o600).unwrap_err().kind,
            FsErrorKind::NotFound
        );
    }
}
