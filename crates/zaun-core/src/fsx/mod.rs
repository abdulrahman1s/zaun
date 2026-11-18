//! Every filesystem access, behind a trait, with the safety helpers trusted code needs.
//!
//! Layer 2. [`Fs`] never follows a symbolic link in the last path component unless the method
//! name ends in `_follow` (ARCHITECTURE §13 invariant 5). `std::fs` is forbidden elsewhere in
//! `zaun-core` by the crate's `clippy.toml`. [`RealFs`] is the std/rustix implementation;
//! `zaun_testkit::MemFs` is the in-memory fake.
//!
//! Helpers built on the trait: [`safe_walk`] (bounded walks that never follow symlinks and
//! stop at mount boundaries), [`safe_remove_tree`] (deletion confined to a root and one
//! filesystem), [`ensure_private_dir`], and [`MountTable`] (`/proc/self/mountinfo`).

mod append;
mod atomic;
mod mounts;
mod real;
mod reflink;
mod safe_walk;

use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub use append::{AppendFile, rotate_numbered};
pub use atomic::atomic_write_real;
pub use mounts::{MountInfo, MountTable};
pub use real::RealFs;
pub use reflink::reflink_probe;
pub use safe_walk::{
    RemoveReport, WalkControl, WalkEntry, WalkLimits, WalkStop, WalkSummary, safe_remove_tree,
    safe_walk,
};

use crate::error::{Error, ErrorCode};

/// The type of a filesystem object (never resolved through a symlink unless a `_follow`
/// method produced it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileType {
    /// Regular file.
    File,
    /// Directory.
    Dir,
    /// Symbolic link.
    Symlink,
    /// Character device.
    CharDevice,
    /// Block device.
    BlockDevice,
    /// FIFO.
    Fifo,
    /// Unix socket.
    Socket,
}

/// Metadata of one filesystem object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Metadata {
    /// Type.
    pub file_type: FileType,
    /// Size in bytes.
    pub size: u64,
    /// Modification time, nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
    /// Inode number.
    pub inode: u64,
    /// Device ID of the containing filesystem.
    pub device: u64,
    /// Permission bits including setuid/setgid/sticky (`mode & 0o7777`).
    pub mode: u32,
    /// Hard-link count.
    pub nlink: u64,
    /// Owner.
    pub uid: u32,
    /// Group.
    pub gid: u32,
    /// Device ID for character and block devices.
    pub rdev: u64,
}

impl Metadata {
    /// A directory.
    pub fn is_dir(&self) -> bool {
        self.file_type == FileType::Dir
    }
    /// A regular file.
    pub fn is_file(&self) -> bool {
        self.file_type == FileType::File
    }
    /// A symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.file_type == FileType::Symlink
    }
}

/// One directory entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DirEntry {
    /// File name.
    pub name: std::ffi::OsString,
    /// Full path.
    pub path: PathBuf,
    /// Type of the entry itself (a symlink is reported as `Symlink`).
    pub file_type: FileType,
}

/// The operation that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FsOp {
    /// `lstat`.
    Metadata,
    /// `stat`.
    MetadataFollow,
    /// `readlink`.
    ReadLink,
    /// Reading a file.
    Read,
    /// Listing a directory.
    ReadDir,
    /// `realpath`.
    Canonicalize,
    /// `mkdir`.
    CreateDir,
    /// Writing a file.
    Write,
    /// `rename`.
    Rename,
    /// `unlink`.
    RemoveFile,
    /// `rmdir`.
    RemoveDir,
    /// `symlink`.
    Symlink,
    /// `chmod`.
    SetMode,
    /// `statvfs`.
    FreeSpace,
    /// `FICLONE`.
    Reflink,
    /// A walk or tree removal.
    Walk,
}

impl FsOp {
    /// Whether the operation modifies the filesystem.
    pub fn is_write(self) -> bool {
        matches!(
            self,
            FsOp::CreateDir
                | FsOp::Write
                | FsOp::Rename
                | FsOp::RemoveFile
                | FsOp::RemoveDir
                | FsOp::Symlink
                | FsOp::SetMode
        )
    }
}

/// Classified failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FsErrorKind {
    /// `ENOENT`.
    NotFound,
    /// `EACCES`, `EPERM`.
    PermissionDenied,
    /// `EEXIST`.
    AlreadyExists,
    /// `ENOTDIR`.
    NotADirectory,
    /// `EISDIR`.
    IsADirectory,
    /// A symlink where following is not allowed (`ELOOP` with `O_NOFOLLOW`), or a symlink
    /// loop.
    Symlink,
    /// `ENOTEMPTY`.
    DirectoryNotEmpty,
    /// `ENOSPC`, `EDQUOT`.
    NoSpace,
    /// `EROFS`.
    ReadOnly,
    /// `EXDEV`, or an operation that would cross a mount boundary.
    CrossDevice,
    /// The file is larger than the read limit.
    TooLarge {
        /// The limit in bytes.
        limit: u64,
    },
    /// A path outside the root an operation is confined to.
    OutsideRoot,
    /// A private directory with the wrong owner, type or mode.
    Insecure,
    /// Invalid argument.
    InvalidInput,
    /// A walk deadline passed.
    TimedOut,
    /// Anything else.
    Other,
}

/// A filesystem error with the operation and the path.
#[derive(Debug)]
pub struct FsError {
    /// The operation.
    pub op: FsOp,
    /// The path.
    pub path: PathBuf,
    /// The classification.
    pub kind: FsErrorKind,
    /// The OS error, when there was one.
    pub source: Option<io::Error>,
}

/// Result of a filesystem operation.
pub type FsResult<T> = Result<T, FsError>;

impl FsError {
    /// A classified error without an OS error.
    pub fn new(op: FsOp, path: impl Into<PathBuf>, kind: FsErrorKind) -> Self {
        Self {
            op,
            path: path.into(),
            kind,
            source: None,
        }
    }

    /// Classify an OS error.
    pub fn from_io(op: FsOp, path: impl Into<PathBuf>, err: io::Error) -> Self {
        let kind = classify(&err);
        Self {
            op,
            path: path.into(),
            kind,
            source: Some(err),
        }
    }

    /// The OS error number, if any.
    pub fn errno(&self) -> Option<i32> {
        self.source.as_ref().and_then(io::Error::raw_os_error)
    }
}

fn classify(err: &io::Error) -> FsErrorKind {
    use rustix::io::Errno;
    if let Some(code) = err.raw_os_error() {
        let e = Errno::from_raw_os_error(code);
        return match e {
            Errno::NOENT => FsErrorKind::NotFound,
            Errno::ACCESS | Errno::PERM => FsErrorKind::PermissionDenied,
            Errno::EXIST => FsErrorKind::AlreadyExists,
            Errno::NOTDIR => FsErrorKind::NotADirectory,
            Errno::ISDIR => FsErrorKind::IsADirectory,
            Errno::LOOP => FsErrorKind::Symlink,
            Errno::NOTEMPTY => FsErrorKind::DirectoryNotEmpty,
            Errno::NOSPC | Errno::DQUOT => FsErrorKind::NoSpace,
            Errno::ROFS => FsErrorKind::ReadOnly,
            Errno::XDEV => FsErrorKind::CrossDevice,
            Errno::INVAL => FsErrorKind::InvalidInput,
            Errno::TIMEDOUT => FsErrorKind::TimedOut,
            _ => FsErrorKind::Other,
        };
    }
    match err.kind() {
        io::ErrorKind::NotFound => FsErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => FsErrorKind::PermissionDenied,
        io::ErrorKind::AlreadyExists => FsErrorKind::AlreadyExists,
        io::ErrorKind::InvalidInput => FsErrorKind::InvalidInput,
        io::ErrorKind::TimedOut => FsErrorKind::TimedOut,
        _ => FsErrorKind::Other,
    }
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {}: {:?}", self.op, self.path.display(), self.kind)?;
        if let Some(e) = &self.source {
            write!(f, " ({e})")?;
        }
        Ok(())
    }
}

impl std::error::Error for FsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

impl From<FsError> for Error {
    fn from(e: FsError) -> Self {
        let code = match e.kind {
            FsErrorKind::NoSpace => ErrorCode::FsDiskFull,
            FsErrorKind::TimedOut => ErrorCode::FsTimeout,
            _ if matches!(e.op, FsOp::RemoveFile | FsOp::RemoveDir | FsOp::Walk) => {
                ErrorCode::FsDeleteIncomplete
            }
            _ if e.op.is_write() => ErrorCode::FsWriteDenied,
            _ => ErrorCode::FsReadDenied,
        };
        let mut err = Error::new(code)
            .with("path", e.path.display())
            .with("op", format!("{:?}", e.op))
            .with("kind", format!("{:?}", e.kind));
        if let Some(errno) = e.errno() {
            err = err.with("errno", errno);
        }
        err.with_source(e)
    }
}

/// Filesystem access. Paths are absolute. Methods never follow a symlink in the last path
/// component unless their name ends in `_follow`; intermediate components are resolved by
/// the kernel as usual (use [`safe_walk`] / [`safe_remove_tree`] inside game-writable trees).
pub trait Fs: Send + Sync {
    /// `lstat`.
    fn metadata(&self, path: &Path) -> FsResult<Metadata>;
    /// `stat` (follows symlinks).
    fn metadata_follow(&self, path: &Path) -> FsResult<Metadata>;
    /// `readlink`.
    fn read_link(&self, path: &Path) -> FsResult<PathBuf>;
    /// Read a whole regular file opened with `O_NOFOLLOW`; fails with
    /// [`FsErrorKind::TooLarge`] when it is longer than `limit` bytes.
    fn read(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>>;
    /// Read like [`Fs::read`] but follow symlinks.
    fn read_follow(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>>;
    /// Read at most `len` bytes starting at `offset` (`O_NOFOLLOW`); shorter at end of file.
    fn read_range(&self, path: &Path, offset: u64, len: usize) -> FsResult<Vec<u8>>;
    /// Open a file for streaming reads (`O_NOFOLLOW`).
    fn open_read(&self, path: &Path) -> FsResult<Box<dyn Read + Send>>;
    /// List a directory, sorted by name. Entry types are not resolved through symlinks.
    fn read_dir(&self, path: &Path) -> FsResult<Vec<DirEntry>>;
    /// Resolve every symlink and return the absolute canonical path.
    fn canonicalize(&self, path: &Path) -> FsResult<PathBuf>;
    /// `mkdir` with `mode`.
    fn create_dir(&self, path: &Path, mode: u32) -> FsResult<()>;
    /// `mkdir -p` with `mode` for created directories.
    fn create_dir_all(&self, path: &Path, mode: u32) -> FsResult<()>;
    /// Create a new file (`O_CREAT | O_EXCL | O_NOFOLLOW`) with `data`.
    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()>;
    /// Replace a file atomically: write a temporary file, `fsync`, rename, `fsync` the
    /// directory. Readers see the old or the new content, never a mix.
    fn atomic_write(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()>;
    /// `rename`.
    fn rename(&self, from: &Path, to: &Path) -> FsResult<()>;
    /// `unlink`; removes a symlink itself, never its target.
    fn remove_file(&self, path: &Path) -> FsResult<()>;
    /// `rmdir` of an empty directory.
    fn remove_dir(&self, path: &Path) -> FsResult<()>;
    /// Create a symlink at `link` pointing to `target`.
    fn symlink(&self, target: &Path, link: &Path) -> FsResult<()>;
    /// Change permission bits; refuses symlinks.
    fn set_mode(&self, path: &Path, mode: u32) -> FsResult<()>;
    /// Bytes available to unprivileged users on the filesystem containing `path`.
    fn free_space(&self, path: &Path) -> FsResult<u64>;
    /// Whether a file in `dir_a` can be reflinked (`FICLONE`) into `dir_b`.
    fn reflink_supported(&self, dir_a: &Path, dir_b: &Path) -> FsResult<bool>;

    /// Whether something exists at `path` (without following a final symlink).
    fn exists(&self, path: &Path) -> bool {
        self.metadata(path).is_ok()
    }

    /// Copy the regular file `from` (never through a symlink) to the new file `to` with
    /// exactly `mode`, as a reflink (`FICLONE`) when the filesystem allows it. Returns whether
    /// the copy is a reflink. The default reads the whole file (fakes); [`RealFs`] streams.
    fn copy_file(&self, from: &Path, to: &Path, mode: u32) -> FsResult<bool> {
        let md = self.metadata(from)?;
        if !md.is_file() {
            return Err(FsError::new(FsOp::Read, from, FsErrorKind::InvalidInput));
        }
        let data = self.read(from, md.size)?;
        self.write_new(to, &data, mode)?;
        self.set_mode(to, mode)?;
        Ok(false)
    }
}

/// Create (mode 0700) or verify a private directory: it must be a real directory (not a
/// symlink), owned by `uid`, and not accessible to group or others. Used for the
/// `/tmp/zaun-<uid>` runtime fallback (CONFIGURATION §5).
pub fn ensure_private_dir(fs: &dyn Fs, path: &Path, uid: u32) -> FsResult<()> {
    match fs.create_dir(path, 0o700) {
        Ok(()) => {}
        Err(e) if e.kind == FsErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let md = fs.metadata(path)?;
    if !md.is_dir() || md.uid != uid || md.mode & 0o077 != 0 {
        return Err(FsError::new(FsOp::Metadata, path, FsErrorKind::Insecure));
    }
    Ok(())
}

/// Whether `path` lies inside `root` (or equals it), comparing normalised components.
pub fn is_within(root: &Path, path: &Path) -> bool {
    let root = crate::paths::normalize(root);
    let path = crate::paths::normalize(path);
    path.starts_with(&root)
}

#[cfg(test)]
mod tests;
