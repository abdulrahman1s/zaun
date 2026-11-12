//! The per-prefix lock `games/<id>/session.lock` (PROC-009, PROCESS_LIFECYCLE §10): an
//! exclusive, non-blocking `flock` held by the supervisor for the whole session and released
//! by the kernel when the supervisor exits, however it exits. A dead holder therefore never
//! leaves a stale lock.

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, Mode, OFlags};

use crate::error::{Error, ErrorCode};
use crate::fsx::{FsError, FsOp};
use crate::launch::PrefixLocks;

/// Why the lock could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    /// Another session holds it (`EP-SES-010`).
    #[error("a session is already running on this prefix")]
    Held,
    /// The lock file could not be opened.
    #[error(transparent)]
    Fs(#[from] FsError),
}

impl From<LockError> for Error {
    fn from(e: LockError) -> Self {
        match e {
            LockError::Held => Error::new(ErrorCode::SesAlreadyRunning),
            LockError::Fs(f) => f.into(),
        }
    }
}

/// A held prefix lock; released on drop or process exit.
#[derive(Debug)]
pub struct PrefixLock {
    _fd: OwnedFd,
    path: PathBuf,
}

fn open(path: &Path) -> Result<OwnedFd, FsError> {
    rustix::fs::open(
        path,
        OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|e| FsError::from_io(FsOp::Write, path, std::io::Error::from(e)))
}

impl PrefixLock {
    /// Take the lock (creating the file, mode 0600, and its directory, mode 0700).
    pub fn try_acquire(path: &Path) -> Result<PrefixLock, LockError> {
        if let Some(dir) = path.parent() {
            crate::fsx::Fs::create_dir_all(&crate::fsx::RealFs::new(), dir, 0o700)?;
        }
        let fd = open(path)?;
        match rustix::fs::flock(&fd, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(PrefixLock {
                _fd: fd,
                path: path.to_path_buf(),
            }),
            Err(rustix::io::Errno::WOULDBLOCK) => Err(LockError::Held),
            Err(e) => Err(FsError::from_io(FsOp::Write, path, e.into()).into()),
        }
    }

    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Whether a live process holds the lock at `path`. Probes with a shared, non-blocking lock
/// that is released at once; a missing file means "free" (nothing is created).
pub fn is_held(path: &Path) -> bool {
    let Ok(fd) = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    ) else {
        return false;
    };
    matches!(
        rustix::fs::flock(&fd, FlockOperation::NonBlockingLockShared),
        Err(rustix::io::Errno::WOULDBLOCK)
    )
}

/// The real [`PrefixLocks`] probe for launch preflight.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlockLocks;

impl PrefixLocks for FlockLocks {
    fn is_held(&self, lock: &Path) -> bool {
        is_held(lock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusive_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("g/session.lock");
        assert!(!is_held(&p));
        let a = PrefixLock::try_acquire(&p).unwrap();
        assert_eq!(a.path(), p);
        assert!(is_held(&p));
        assert!(matches!(PrefixLock::try_acquire(&p), Err(LockError::Held)));
        assert!(FlockLocks.is_held(&p));
        drop(a);
        assert!(!is_held(&p));
        let _b = PrefixLock::try_acquire(&p).unwrap();
    }
}
