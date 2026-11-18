//! Failure injection by counting pass (TEST-002, TESTING_STRATEGY §10.2).
//!
//! [`FaultFs`] wraps any [`Fs`] (normally a [`MemFs`](crate::MemFs)) and numbers every call
//! of a chosen class: a first run of a flow with nothing armed counts the call sites it
//! reaches ([`FaultFs::count`]); then the flow is run again once per call site and fault
//! kind with [`FaultFs::arm`] failing exactly that call. [`fault_matrix`] drives the whole
//! loop, so the matrix covers every I/O call site of a flow without listing them by hand.
//!
//! ```
//! use std::path::Path;
//! use std::sync::Arc;
//! use zaun_core::fsx::{Fs, FsErrorKind};
//! use zaun_testkit::MemFs;
//! use zaun_testkit::inject::{Calls, FaultFs, fault_matrix};
//!
//! let report = fault_matrix(
//!     Calls::Mutations,
//!     &[FsErrorKind::NoSpace, FsErrorKind::ReadOnly],
//!     || FaultFs::new(Arc::new(MemFs::new())),
//!     |fs| {
//!         fs.create_dir_all(Path::new("/a/b"), 0o700)?;
//!         fs.atomic_write(Path::new("/a/b/f"), b"x", 0o600)
//!     },
//!     |fs, result| {
//!         // Consistency: the file is complete or absent.
//!         if result.is_err() {
//!             assert!(fs.read(Path::new("/a/b/f"), 10).is_err());
//!         }
//!     },
//! );
//! assert!(report.sites >= 2);
//! assert_eq!(report.runs, report.sites * 2);
//! ```

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use zaun_core::fsx::{DirEntry, Fs, FsError, FsErrorKind, FsOp, FsResult, Metadata};

/// Which calls are numbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calls {
    /// Writes, creations, renames, removals, mode changes, copies (TESTING_STRATEGY §10.2
    /// "every `Fs` write and create call site").
    Mutations,
    /// Reads, listings and `stat`s (for "slow filesystem" and `EIO` on reads).
    Reads,
    /// Everything.
    All,
}

impl Calls {
    fn covers(self, op: FsOp) -> bool {
        let mutation = matches!(
            op,
            FsOp::CreateDir
                | FsOp::Write
                | FsOp::Rename
                | FsOp::RemoveFile
                | FsOp::RemoveDir
                | FsOp::Symlink
                | FsOp::SetMode
                | FsOp::Reflink
        );
        match self {
            Calls::Mutations => mutation,
            Calls::Reads => !mutation,
            Calls::All => true,
        }
    }
}

#[derive(Debug)]
struct State {
    class: Calls,
    seen: usize,
    armed: Option<(usize, FsErrorKind)>,
    log: Vec<(FsOp, PathBuf)>,
    fired: Option<(FsOp, PathBuf)>,
}

/// An [`Fs`] that numbers calls of one class and fails the armed one.
pub struct FaultFs {
    inner: Arc<dyn Fs>,
    state: Mutex<State>,
}

impl std::fmt::Debug for FaultFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaultFs")
            .field("state", &*self.state.lock())
            .finish_non_exhaustive()
    }
}

impl FaultFs {
    /// Wrap `inner`, numbering [`Calls::Mutations`].
    pub fn new(inner: Arc<dyn Fs>) -> Self {
        Self::with_class(inner, Calls::Mutations)
    }

    /// Wrap `inner`, numbering `class`.
    pub fn with_class(inner: Arc<dyn Fs>, class: Calls) -> Self {
        Self {
            inner,
            state: Mutex::new(State {
                class,
                seen: 0,
                armed: None,
                log: Vec::new(),
                fired: None,
            }),
        }
    }

    /// The wrapped filesystem.
    pub fn inner(&self) -> &Arc<dyn Fs> {
        &self.inner
    }

    /// Fail the `n`-th numbered call (1-based) from now on with `kind`, and restart counting.
    pub fn arm(&self, n: usize, kind: FsErrorKind) {
        let mut s = self.state.lock();
        s.seen = 0;
        s.armed = Some((n, kind));
        s.fired = None;
        s.log.clear();
    }

    /// Stop failing (the "fault removed" step before a retry).
    pub fn disarm(&self) {
        self.state.lock().armed = None;
    }

    /// Numbered calls since construction or the last [`FaultFs::arm`].
    pub fn count(&self) -> usize {
        self.state.lock().seen
    }

    /// Numbered calls in order.
    pub fn log(&self) -> Vec<(FsOp, PathBuf)> {
        self.state.lock().log.clone()
    }

    /// The call the armed fault hit, if it was reached.
    pub fn fired(&self) -> Option<(FsOp, PathBuf)> {
        self.state.lock().fired.clone()
    }

    fn check(&self, op: FsOp, path: &Path) -> FsResult<()> {
        let mut s = self.state.lock();
        if !s.class.covers(op) {
            return Ok(());
        }
        s.seen += 1;
        s.log.push((op, path.to_owned()));
        match s.armed {
            Some((n, kind)) if n == s.seen => {
                s.fired = Some((op, path.to_owned()));
                Err(FsError::new(op, path, kind))
            }
            _ => Ok(()),
        }
    }
}

impl Fs for FaultFs {
    fn metadata(&self, path: &Path) -> FsResult<Metadata> {
        self.check(FsOp::Metadata, path)?;
        self.inner.metadata(path)
    }
    fn metadata_follow(&self, path: &Path) -> FsResult<Metadata> {
        self.check(FsOp::MetadataFollow, path)?;
        self.inner.metadata_follow(path)
    }
    fn read_link(&self, path: &Path) -> FsResult<PathBuf> {
        self.check(FsOp::ReadLink, path)?;
        self.inner.read_link(path)
    }
    fn read(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        self.check(FsOp::Read, path)?;
        self.inner.read(path, limit)
    }
    fn read_follow(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        self.check(FsOp::Read, path)?;
        self.inner.read_follow(path, limit)
    }
    fn read_range(&self, path: &Path, offset: u64, len: usize) -> FsResult<Vec<u8>> {
        self.check(FsOp::Read, path)?;
        self.inner.read_range(path, offset, len)
    }
    fn open_read(&self, path: &Path) -> FsResult<Box<dyn Read + Send>> {
        self.check(FsOp::Read, path)?;
        self.inner.open_read(path)
    }
    fn read_dir(&self, path: &Path) -> FsResult<Vec<DirEntry>> {
        self.check(FsOp::ReadDir, path)?;
        self.inner.read_dir(path)
    }
    fn canonicalize(&self, path: &Path) -> FsResult<PathBuf> {
        self.check(FsOp::Canonicalize, path)?;
        self.inner.canonicalize(path)
    }
    fn create_dir(&self, path: &Path, mode: u32) -> FsResult<()> {
        self.check(FsOp::CreateDir, path)?;
        self.inner.create_dir(path, mode)
    }
    fn create_dir_all(&self, path: &Path, mode: u32) -> FsResult<()> {
        self.check(FsOp::CreateDir, path)?;
        self.inner.create_dir_all(path, mode)
    }
    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        self.check(FsOp::Write, path)?;
        self.inner.write_new(path, data, mode)
    }
    fn atomic_write(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        self.check(FsOp::Write, path)?;
        self.inner.atomic_write(path, data, mode)
    }
    fn rename(&self, from: &Path, to: &Path) -> FsResult<()> {
        self.check(FsOp::Rename, from)?;
        self.inner.rename(from, to)
    }
    fn remove_file(&self, path: &Path) -> FsResult<()> {
        self.check(FsOp::RemoveFile, path)?;
        self.inner.remove_file(path)
    }
    fn remove_dir(&self, path: &Path) -> FsResult<()> {
        self.check(FsOp::RemoveDir, path)?;
        self.inner.remove_dir(path)
    }
    fn symlink(&self, target: &Path, link: &Path) -> FsResult<()> {
        self.check(FsOp::Symlink, link)?;
        self.inner.symlink(target, link)
    }
    fn set_mode(&self, path: &Path, mode: u32) -> FsResult<()> {
        self.check(FsOp::SetMode, path)?;
        self.inner.set_mode(path, mode)
    }
    fn free_space(&self, path: &Path) -> FsResult<u64> {
        self.check(FsOp::FreeSpace, path)?;
        self.inner.free_space(path)
    }
    fn reflink_supported(&self, dir_a: &Path, dir_b: &Path) -> FsResult<bool> {
        self.check(FsOp::Reflink, dir_a)?;
        self.inner.reflink_supported(dir_a, dir_b)
    }
    fn copy_file(&self, from: &Path, to: &Path, mode: u32) -> FsResult<bool> {
        self.check(FsOp::Write, to)?;
        self.inner.copy_file(from, to, mode)
    }
}

/// What [`fault_matrix`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixReport {
    /// Call sites the counting pass found.
    pub sites: usize,
    /// Faulted runs (sites × kinds).
    pub runs: usize,
    /// Runs in which the flow still succeeded (the fault hit a call whose failure the flow
    /// tolerates, such as best-effort cleanup).
    pub tolerated: usize,
    /// The calls of the counting pass.
    pub log: Vec<(FsOp, PathBuf)>,
}

/// Run `flow` once on a fresh world to count its numbered calls, then once per call site and
/// fault kind with that call failing. `check(world, result)` asserts consistency after every
/// run (including the counting pass).
pub fn fault_matrix<W, T, E>(
    class: Calls,
    kinds: &[FsErrorKind],
    mut world: impl FnMut() -> W,
    mut flow: impl FnMut(&W) -> Result<T, E>,
    mut check: impl FnMut(&W, &Result<T, E>),
) -> MatrixReport
where
    W: AsRef<FaultFs>,
{
    let w = world();
    let fs = w.as_ref();
    {
        let mut s = fs.state.lock();
        s.class = class;
        s.seen = 0;
        s.log.clear();
    }
    let r = flow(&w);
    check(&w, &r);
    assert!(r.is_ok(), "the flow fails without any injected fault");
    let sites = w.as_ref().count();
    let log = w.as_ref().log();
    let mut runs = 0;
    let mut tolerated = 0;
    for n in 1..=sites {
        for &kind in kinds {
            let w = world();
            w.as_ref().state.lock().class = class;
            w.as_ref().arm(n, kind);
            let r = flow(&w);
            assert!(
                w.as_ref().fired().is_some(),
                "call {n} ({:?}) was not reached on the faulted run",
                log.get(n - 1)
            );
            if r.is_ok() {
                tolerated += 1;
            }
            check(&w, &r);
            runs += 1;
        }
    }
    MatrixReport {
        sites,
        runs,
        tolerated,
        log,
    }
}

impl AsRef<FaultFs> for FaultFs {
    fn as_ref(&self) -> &FaultFs {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemFs;

    #[test]
    fn arms_the_nth_call_only() {
        let fs = FaultFs::new(Arc::new(MemFs::new()));
        fs.arm(2, FsErrorKind::NoSpace);
        fs.create_dir(Path::new("/a"), 0o700).unwrap();
        let e = fs.create_dir(Path::new("/b"), 0o700).unwrap_err();
        assert_eq!(e.kind, FsErrorKind::NoSpace);
        fs.create_dir(Path::new("/c"), 0o700).unwrap();
        assert_eq!(fs.count(), 3);
        assert_eq!(fs.fired().unwrap().1, PathBuf::from("/b"));
        // Reads are not numbered in the mutation class.
        assert!(fs.metadata(Path::new("/a")).is_ok());
        assert_eq!(fs.count(), 3);
        fs.disarm();
    }
}
