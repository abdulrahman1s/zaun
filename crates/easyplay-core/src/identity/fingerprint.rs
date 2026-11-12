//! Size, modification time, quick hash and optional full hash (GAME_IDENTITY §3).

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256 as Sha256Hasher};

use crate::consts::QUICK_HASH_WINDOW;
use crate::fsx::{FileType, Fs, FsError, FsErrorKind, FsOp, FsResult, Metadata};
use crate::jobs::CancelToken;
use crate::model::{Fingerprint, Sha256};

/// Files larger than this never get a full hash (GAME_IDENTITY §3).
pub const FULL_HASH_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// Read size of the streaming full hash.
const FULL_HASH_CHUNK: usize = 1024 * 1024;

/// The quick hash of a file of `size` bytes whose first window is `head` and whose last
/// window (only when `size > 2 × QUICK_HASH_WINDOW`) is `tail`.
///
/// SHA-256 over the 8-byte little-endian size, `head`, then `tail`.
pub fn quick_hash_of(size: u64, head: &[u8], tail: Option<&[u8]>) -> Sha256 {
    let mut h = Sha256Hasher::new();
    h.update(size.to_le_bytes());
    h.update(head);
    if let Some(t) = tail {
        h.update(t);
    }
    h.finalize().into()
}

/// The byte ranges the quick hash reads for a file of `size` bytes: `(offset, len)` of the
/// head window and, for files larger than two windows, of the tail window.
pub fn quick_hash_windows(size: u64) -> ((u64, u64), Option<(u64, u64)>) {
    let head = (0, size.min(QUICK_HASH_WINDOW));
    let tail =
        (size > 2 * QUICK_HASH_WINDOW).then(|| (size - QUICK_HASH_WINDOW, QUICK_HASH_WINDOW));
    (head, tail)
}

fn read_exact_range(fs: &dyn Fs, path: &Path, offset: u64, len: u64) -> FsResult<Vec<u8>> {
    let want = usize::try_from(len).unwrap_or(usize::MAX);
    let data = fs.read_range(path, offset, want)?;
    if data.len() != want {
        // The file shrank between `lstat` and the read: the fingerprint would be wrong.
        return Err(FsError::new(FsOp::Read, path, FsErrorKind::Other));
    }
    Ok(data)
}

/// The quick hash of `path`, whose `lstat` size is `size`.
pub fn quick_hash(fs: &dyn Fs, path: &Path, size: u64) -> FsResult<Sha256> {
    let ((ho, hl), tail) = quick_hash_windows(size);
    let head = read_exact_range(fs, path, ho, hl)?;
    let tail = match tail {
        Some((to, tl)) => Some(read_exact_range(fs, path, to, tl)?),
        None => None,
    };
    Ok(quick_hash_of(size, &head, tail.as_deref()))
}

fn regular_file(fs: &dyn Fs, path: &Path) -> FsResult<Metadata> {
    let md = fs.metadata(path)?;
    match md.file_type {
        FileType::File => Ok(md),
        FileType::Symlink => Err(FsError::new(FsOp::Metadata, path, FsErrorKind::Symlink)),
        FileType::Dir => Err(FsError::new(
            FsOp::Metadata,
            path,
            FsErrorKind::IsADirectory,
        )),
        _ => Err(FsError::new(
            FsOp::Metadata,
            path,
            FsErrorKind::InvalidInput,
        )),
    }
}

/// Size, mtime and quick hash of the regular file at `path` (never through a final
/// symlink). `full_hash` is left empty; see [`full_hash`].
pub fn compute(fs: &dyn Fs, path: &Path) -> FsResult<Fingerprint> {
    let md = regular_file(fs, path)?;
    let quick_hash = quick_hash(fs, path, md.size)?;
    Ok(Fingerprint {
        size: md.size,
        mtime_ns: md.mtime_ns,
        quick_hash,
        full_hash: None,
    })
}

/// The "unchanged" shortcut used on every launch: same size and mtime as `stored` means the
/// stored fingerprint is trusted without reading the file.
pub fn is_unchanged(stored: &Fingerprint, md: &Metadata) -> bool {
    stored.size == md.size && stored.mtime_ns == md.mtime_ns
}

/// Return `stored` when the file is unchanged by the size-and-mtime test, otherwise a fresh
/// fingerprint (the full hash is dropped because it no longer applies).
pub fn refresh(fs: &dyn Fs, path: &Path, stored: &Fingerprint) -> FsResult<(Fingerprint, bool)> {
    let md = regular_file(fs, path)?;
    if is_unchanged(stored, &md) {
        return Ok((*stored, false));
    }
    let quick_hash = quick_hash(fs, path, md.size)?;
    Ok((
        Fingerprint {
            size: md.size,
            mtime_ns: md.mtime_ns,
            quick_hash,
            full_hash: None,
        },
        true,
    ))
}

/// Outcome of [`full_hash`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullHash {
    /// The digest.
    Done(Sha256),
    /// The file is larger than the limit; no hash.
    TooLarge,
    /// The token was cancelled.
    Cancelled,
    /// The file changed size while it was read.
    Changed,
}

/// SHA-256 of the whole file, for files of at most `limit` bytes (normally
/// [`FULL_HASH_MAX_BYTES`]); checks `cancel` between 1 MiB reads. Background job only.
pub fn full_hash(fs: &dyn Fs, path: &Path, limit: u64, cancel: &CancelToken) -> FsResult<FullHash> {
    let md = regular_file(fs, path)?;
    if md.size > limit {
        return Ok(FullHash::TooLarge);
    }
    let mut reader = fs.open_read(path)?;
    let mut h = Sha256Hasher::new();
    let mut buf = vec![0u8; FULL_HASH_CHUNK];
    let mut total: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            return Ok(FullHash::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| FsError::from_io(FsOp::Read, path, e))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > md.size {
            return Ok(FullHash::Changed);
        }
        h.update(&buf[..n]);
    }
    if total != md.size {
        return Ok(FullHash::Changed);
    }
    Ok(FullHash::Done(h.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u64 = QUICK_HASH_WINDOW;

    #[test]
    fn windows_at_boundaries() {
        assert_eq!(quick_hash_windows(0), ((0, 0), None));
        assert_eq!(quick_hash_windows(10), ((0, 10), None));
        assert_eq!(quick_hash_windows(W), ((0, W), None));
        assert_eq!(quick_hash_windows(W + 1), ((0, W), None));
        assert_eq!(quick_hash_windows(2 * W), ((0, W), None));
        assert_eq!(quick_hash_windows(2 * W + 1), ((0, W), Some((W + 1, W))));
        assert_eq!(quick_hash_windows(5 * W), ((0, W), Some((4 * W, W))));
    }

    #[test]
    fn size_is_part_of_the_hash() {
        assert_ne!(quick_hash_of(1, b"", None), quick_hash_of(2, b"", None));
        let expected: Sha256 = Sha256Hasher::digest(3u64.to_le_bytes()).into();
        assert_eq!(quick_hash_of(3, b"", None), expected);
    }
}
