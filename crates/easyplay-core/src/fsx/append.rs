//! Append-only files shared by several processes (the application log, LOGGING §2.1).
//!
//! Each record is written with a single `write` on an `O_APPEND` descriptor, so concurrent
//! writers never interleave within a record. Rotation renames numbered files under an
//! exclusive `flock`; writers notice that their descriptor no longer refers to the path and
//! reopen.

#![allow(clippy::disallowed_methods)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::FlockOperation;

/// An `O_APPEND` file handle that can detect rotation by another process.
#[derive(Debug)]
pub struct AppendFile {
    file: File,
    path: PathBuf,
    mode: u32,
    ino: u64,
    dev: u64,
}

impl AppendFile {
    /// Open or create `path` (`O_APPEND | O_CREAT | O_NOFOLLOW`).
    pub fn open(path: &Path, mode: u32) -> io::Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(mode)
            .custom_flags(super::real::o_nofollow())
            .open(path)?;
        let md = file.metadata()?;
        Ok(Self {
            file,
            path: path.to_owned(),
            mode,
            ino: md.ino(),
            dev: md.dev(),
        })
    }

    /// The path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write one record with a single `write_all` (one `write` for records of normal size).
    pub fn write_record(&mut self, record: &[u8]) -> io::Result<()> {
        self.file.write_all(record)
    }

    /// Current length of the open file.
    pub fn len(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    /// Whether the open file is empty.
    pub fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Whether `path` still names the open file (false after another process rotated it).
    pub fn is_current(&self) -> bool {
        fs::symlink_metadata(&self.path)
            .map(|m| m.ino() == self.ino && m.dev() == self.dev)
            .unwrap_or(false)
    }

    /// Reopen `path`.
    pub fn reopen(&mut self) -> io::Result<()> {
        *self = Self::open(&self.path, self.mode)?;
        Ok(())
    }

    /// Run `f` while holding an exclusive `flock` on the open file.
    pub fn with_lock<T>(&self, f: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        rustix::fs::flock(&self.file, FlockOperation::LockExclusive).map_err(io::Error::from)?;
        let result = f();
        let _ = rustix::fs::flock(&self.file, FlockOperation::Unlock);
        result
    }
}

/// Shift numbered files: delete `path.(keep-1)`, rename `path.n` → `path.(n+1)` and `path` →
/// `path.1`. Missing files are skipped. With `keep` = 5 the files are `path`, `.1` … `.4`.
pub fn rotate_numbered(path: &Path, keep: usize) -> io::Result<()> {
    let numbered = |n: usize| {
        let mut s = path.as_os_str().to_owned();
        s.push(format!(".{n}"));
        PathBuf::from(s)
    };
    if keep <= 1 {
        return match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    match fs::remove_file(numbered(keep - 1)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    for n in (1..keep - 1).rev() {
        match fs::rename(numbered(n), numbered(n + 1)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    match fs::rename(path, numbered(1)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_detect_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("easyplay.log");
        let mut a = AppendFile::open(&p, 0o600).unwrap();
        let mut b = AppendFile::open(&p, 0o600).unwrap();
        a.write_record(b"one\n").unwrap();
        b.write_record(b"two\n").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one\ntwo\n");
        assert_eq!(a.len().unwrap(), 8);
        assert!(a.is_current());
        a.with_lock(|| rotate_numbered(&p, 5)).unwrap();
        assert!(!a.is_current());
        assert!(!b.is_current());
        a.reopen().unwrap();
        a.write_record(b"three\n").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"three\n");
        assert_eq!(
            fs::read(dir.path().join("easyplay.log.1")).unwrap(),
            b"one\ntwo\n"
        );
    }

    #[test]
    fn rotation_keeps_n_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.log");
        for i in 0..8 {
            fs::write(&p, format!("{i}")).unwrap();
            rotate_numbered(&p, 5).unwrap();
        }
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["x.log.1", "x.log.2", "x.log.3", "x.log.4"]);
        assert_eq!(fs::read_to_string(dir.path().join("x.log.1")).unwrap(), "7");
        assert_eq!(fs::read_to_string(dir.path().join("x.log.4")).unwrap(), "4");
    }

    #[test]
    fn refuses_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(AppendFile::open(&link, 0o600).is_err());
    }
}
