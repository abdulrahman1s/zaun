//! `FICLONE` probe: can a file in one directory be reflinked into another
//! (RUNTIME_MANAGEMENT §7)?

#![allow(clippy::disallowed_methods)]

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use rustix::io::Errno;

/// Create a small file in `dir_a`, try to clone it into a new file in `dir_b`, clean up.
/// `Ok(false)` when the filesystems do not support reflinks or differ; `Err` for other
/// failures (e.g. the directories are not writable).
pub fn reflink_probe(dir_a: &Path, dir_b: &Path) -> io::Result<bool> {
    let name = format!(".zaun-reflink-probe-{}", std::process::id());
    let src_path = dir_a.join(format!("{name}.src"));
    let dst_path = dir_b.join(format!("{name}.dst"));
    let result = (|| -> io::Result<bool> {
        let mut src = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(super::real::o_nofollow())
            .open(&src_path)?;
        src.write_all(&[0x5a; 4096])?;
        src.sync_all()?;
        let dst = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(super::real::o_nofollow())
            .open(&dst_path)?;
        match rustix::fs::ioctl_ficlone(&dst, &src) {
            Ok(()) => Ok(true),
            Err(
                Errno::OPNOTSUPP
                | Errno::XDEV
                | Errno::INVAL
                | Errno::NOTTY
                | Errno::NOSYS
                | Errno::BADF
                | Errno::PERM,
            ) => Ok(false),
            Err(e) => Err(io::Error::from(e)),
        }
    })();
    let _ = fs::remove_file(&src_path);
    let _ = fs::remove_file(&dst_path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_runs_and_cleans_up() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        // The answer depends on the test machine's filesystem; it must not error.
        let _supported = reflink_probe(a.path(), b.path()).unwrap();
        assert_eq!(fs::read_dir(a.path()).unwrap().count(), 0);
        assert_eq!(fs::read_dir(b.path()).unwrap().count(), 0);
    }

    #[test]
    fn unwritable_directory_is_an_error() {
        let a = tempfile::tempdir().unwrap();
        assert!(reflink_probe(&a.path().join("missing"), a.path()).is_err());
    }
}
