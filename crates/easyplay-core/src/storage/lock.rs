//! The cross-process `library.lock` (PERSISTENCE §6).
//!
//! An advisory `flock` coordinating library-table writers: the GUI holds it shared for its
//! lifetime and exclusively only for migration; a headless CLI that mutates the library holds
//! it exclusively. Acquisition never blocks: a conflicting holder yields
//! [`StorageError::LockHeld`]. The lock is released when the guard is dropped (or the
//! process dies). Locks belong to the open file description, so two guards in one process
//! contend exactly like two processes.

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, Mode, OFlags};

use super::{StorageError, StorageResult};
use crate::fsx::{FsError, FsOp};

/// Shared or exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LockMode {
    /// Several holders (the GUI).
    Shared,
    /// One holder (migration, headless library changes).
    Exclusive,
}

/// A held `library.lock`; released on drop.
#[derive(Debug)]
pub struct LibraryLock {
    fd: OwnedFd,
    path: PathBuf,
    mode: LockMode,
}

impl LibraryLock {
    /// Try to take the lock at `path` (created with mode 0600 if missing).
    pub fn try_acquire(path: &Path, mode: LockMode) -> StorageResult<LibraryLock> {
        let io = |e: rustix::io::Errno| {
            StorageError::Fs(FsError::from_io(FsOp::Write, path, std::io::Error::from(e)))
        };
        let fd = rustix::fs::open(
            path,
            OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        )
        .map_err(io)?;
        flock(&fd, mode).map_err(|e| {
            if e == rustix::io::Errno::WOULDBLOCK {
                StorageError::LockHeld
            } else {
                io(e)
            }
        })?;
        Ok(LibraryLock {
            fd,
            path: path.to_path_buf(),
            mode,
        })
    }

    /// Change the mode without releasing (shared ↔ exclusive). On failure the previous mode
    /// may have been dropped to shared by the kernel; the guard reports the mode it holds.
    pub fn convert(&mut self, mode: LockMode) -> StorageResult<()> {
        if mode == self.mode {
            return Ok(());
        }
        match flock(&self.fd, mode) {
            Ok(()) => {
                self.mode = mode;
                Ok(())
            }
            Err(rustix::io::Errno::WOULDBLOCK) => Err(StorageError::LockHeld),
            Err(e) => Err(StorageError::Fs(FsError::from_io(
                FsOp::Write,
                &self.path,
                std::io::Error::from(e),
            ))),
        }
    }

    /// The mode held.
    pub fn mode(&self) -> LockMode {
        self.mode
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn flock(fd: &OwnedFd, mode: LockMode) -> rustix::io::Result<()> {
    let op = match mode {
        LockMode::Shared => FlockOperation::NonBlockingLockShared,
        LockMode::Exclusive => FlockOperation::NonBlockingLockExclusive,
    };
    rustix::fs::flock(fd, op)
}

impl Drop for LibraryLock {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(&self.fd, FlockOperation::Unlock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_exclusive_acquirer_fails_fast() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.lock");
        let held = LibraryLock::try_acquire(&path, LockMode::Exclusive).unwrap();
        assert!(matches!(
            LibraryLock::try_acquire(&path, LockMode::Exclusive),
            Err(StorageError::LockHeld)
        ));
        assert!(matches!(
            LibraryLock::try_acquire(&path, LockMode::Shared),
            Err(StorageError::LockHeld)
        ));
        drop(held);
        LibraryLock::try_acquire(&path, LockMode::Exclusive).unwrap();
    }

    #[test]
    fn shared_holders_coexist_and_block_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.lock");
        let mut a = LibraryLock::try_acquire(&path, LockMode::Shared).unwrap();
        let b = LibraryLock::try_acquire(&path, LockMode::Shared).unwrap();
        assert!(matches!(
            a.convert(LockMode::Exclusive),
            Err(StorageError::LockHeld)
        ));
        drop(b);
        a.convert(LockMode::Exclusive).unwrap();
        assert_eq!(a.mode(), LockMode::Exclusive);
        assert!(matches!(
            LibraryLock::try_acquire(&path, LockMode::Shared),
            Err(StorageError::LockHeld)
        ));
    }
}
