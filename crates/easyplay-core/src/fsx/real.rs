//! The std/rustix implementation of [`Fs`].

// This module is one of the places allowed to use `std::fs` (crate `clippy.toml`).
#![allow(clippy::disallowed_methods)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::ui_thread;

use super::{
    DirEntry, FileType, Fs, FsError, FsErrorKind, FsOp, FsResult, Metadata, atomic_write_real,
    reflink_probe,
};

/// `O_NOFOLLOW` as accepted by `OpenOptionsExt::custom_flags`.
pub(super) fn o_nofollow() -> i32 {
    // The flag value fits in i32 on every Linux architecture.
    rustix::fs::OFlags::NOFOLLOW.bits() as i32
}

/// The real filesystem.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealFs;

impl RealFs {
    /// A new handle (stateless).
    pub fn new() -> Self {
        Self
    }
}

pub(super) fn convert_metadata(md: &fs::Metadata) -> Metadata {
    let ft = md.file_type();
    use std::os::unix::fs::FileTypeExt;
    let file_type = if ft.is_symlink() {
        FileType::Symlink
    } else if ft.is_dir() {
        FileType::Dir
    } else if ft.is_file() {
        FileType::File
    } else if ft.is_char_device() {
        FileType::CharDevice
    } else if ft.is_block_device() {
        FileType::BlockDevice
    } else if ft.is_fifo() {
        FileType::Fifo
    } else {
        FileType::Socket
    };
    Metadata {
        file_type,
        size: md.size(),
        mtime_ns: md
            .mtime()
            .saturating_mul(1_000_000_000)
            .saturating_add(md.mtime_nsec()),
        inode: md.ino(),
        device: md.dev(),
        mode: md.mode() & 0o7777,
        nlink: md.nlink(),
        uid: md.uid(),
        gid: md.gid(),
        rdev: md.rdev(),
    }
}

/// The type of a listed entry, or `None` when the entry is gone again. A directory can
/// change while it is listed; `/proc` lists a process that exits at that moment without a
/// type, and looking it up then finds nothing. Such an entry is not in the directory any
/// more: failing the whole listing for it would, for one, empty the supervisor's process
/// lists whenever some unrelated process ends at the wrong time.
pub(super) fn listed_type(looked_up: io::Result<fs::FileType>) -> io::Result<Option<fs::FileType>> {
    match looked_up {
        Ok(ft) => Ok(Some(ft)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn file_type_of(ft: fs::FileType) -> FileType {
    use std::os::unix::fs::FileTypeExt;
    if ft.is_symlink() {
        FileType::Symlink
    } else if ft.is_dir() {
        FileType::Dir
    } else if ft.is_file() {
        FileType::File
    } else if ft.is_char_device() {
        FileType::CharDevice
    } else if ft.is_block_device() {
        FileType::BlockDevice
    } else if ft.is_fifo() {
        FileType::Fifo
    } else {
        FileType::Socket
    }
}

fn open_nofollow(path: &Path) -> FsResult<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(o_nofollow())
        .open(path)
        .map_err(|e| FsError::from_io(FsOp::Read, path, e))?;
    let md = file
        .metadata()
        .map_err(|e| FsError::from_io(FsOp::Read, path, e))?;
    if md.is_dir() {
        return Err(FsError::new(FsOp::Read, path, FsErrorKind::IsADirectory));
    }
    Ok(file)
}

fn read_limited(mut file: File, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
    let mut buf = Vec::new();
    (&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| FsError::from_io(FsOp::Read, path, e))?;
    if buf.len() as u64 > limit {
        return Err(FsError::new(
            FsOp::Read,
            path,
            FsErrorKind::TooLarge { limit },
        ));
    }
    Ok(buf)
}

// Every blocking operation asserts it is not on the UI thread (PERF-002); `metadata`,
// `metadata_follow`, `read_link` and `canonicalize` are exempt: the policy editor's
// validation runs them on the UI thread (crate::ui_thread).
impl Fs for RealFs {
    fn metadata(&self, path: &Path) -> FsResult<Metadata> {
        fs::symlink_metadata(path)
            .map(|m| convert_metadata(&m))
            .map_err(|e| FsError::from_io(FsOp::Metadata, path, e))
    }

    fn metadata_follow(&self, path: &Path) -> FsResult<Metadata> {
        fs::metadata(path)
            .map(|m| convert_metadata(&m))
            .map_err(|e| FsError::from_io(FsOp::MetadataFollow, path, e))
    }

    fn read_link(&self, path: &Path) -> FsResult<PathBuf> {
        fs::read_link(path).map_err(|e| FsError::from_io(FsOp::ReadLink, path, e))
    }

    fn read(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        ui_thread::assert_not_ui_thread("Fs::read");
        let file = open_nofollow(path)?;
        read_limited(file, path, limit)
    }

    fn read_follow(&self, path: &Path, limit: u64) -> FsResult<Vec<u8>> {
        ui_thread::assert_not_ui_thread("Fs::read_follow");
        let file = File::open(path).map_err(|e| FsError::from_io(FsOp::Read, path, e))?;
        read_limited(file, path, limit)
    }

    fn read_range(&self, path: &Path, offset: u64, len: usize) -> FsResult<Vec<u8>> {
        ui_thread::assert_not_ui_thread("Fs::read_range");
        let mut file = open_nofollow(path)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| FsError::from_io(FsOp::Read, path, e))?;
        let mut buf = Vec::with_capacity(len.min(16 * 1024 * 1024));
        file.take(len as u64)
            .read_to_end(&mut buf)
            .map_err(|e| FsError::from_io(FsOp::Read, path, e))?;
        Ok(buf)
    }

    fn open_read(&self, path: &Path) -> FsResult<Box<dyn Read + Send>> {
        ui_thread::assert_not_ui_thread("Fs::open_read");
        Ok(Box::new(open_nofollow(path)?))
    }

    fn read_dir(&self, path: &Path) -> FsResult<Vec<DirEntry>> {
        ui_thread::assert_not_ui_thread("Fs::read_dir");
        let iter = fs::read_dir(path).map_err(|e| FsError::from_io(FsOp::ReadDir, path, e))?;
        let mut out = Vec::new();
        for entry in iter {
            let entry = entry.map_err(|e| FsError::from_io(FsOp::ReadDir, path, e))?;
            let Some(ft) = listed_type(entry.file_type())
                .map_err(|e| FsError::from_io(FsOp::ReadDir, entry.path(), e))?
            else {
                continue;
            };
            out.push(DirEntry {
                name: entry.file_name(),
                path: entry.path(),
                file_type: file_type_of(ft),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn canonicalize(&self, path: &Path) -> FsResult<PathBuf> {
        fs::canonicalize(path).map_err(|e| FsError::from_io(FsOp::Canonicalize, path, e))
    }

    fn create_dir(&self, path: &Path, mode: u32) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::create_dir");
        fs::DirBuilder::new()
            .mode(mode)
            .create(path)
            .map_err(|e| FsError::from_io(FsOp::CreateDir, path, e))
    }

    fn create_dir_all(&self, path: &Path, mode: u32) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::create_dir_all");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(mode)
            .create(path)
            .map_err(|e| FsError::from_io(FsOp::CreateDir, path, e))
    }

    fn write_new(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::write_new");
        use std::io::Write;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(o_nofollow())
            .open(path)
            .map_err(|e| FsError::from_io(FsOp::Write, path, e))?;
        file.write_all(data)
            .and_then(|()| file.sync_all())
            .map_err(|e| FsError::from_io(FsOp::Write, path, e))
    }

    fn atomic_write(&self, path: &Path, data: &[u8], mode: u32) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::atomic_write");
        atomic_write_real(path, data, mode)
    }

    fn rename(&self, from: &Path, to: &Path) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::rename");
        fs::rename(from, to).map_err(|e| FsError::from_io(FsOp::Rename, from, e))
    }

    fn remove_file(&self, path: &Path) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::remove_file");
        fs::remove_file(path).map_err(|e| FsError::from_io(FsOp::RemoveFile, path, e))
    }

    fn remove_dir(&self, path: &Path) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::remove_dir");
        fs::remove_dir(path).map_err(|e| FsError::from_io(FsOp::RemoveDir, path, e))
    }

    fn symlink(&self, target: &Path, link: &Path) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::symlink");
        std::os::unix::fs::symlink(target, link)
            .map_err(|e| FsError::from_io(FsOp::Symlink, link, e))
    }

    fn set_mode(&self, path: &Path, mode: u32) -> FsResult<()> {
        ui_thread::assert_not_ui_thread("Fs::set_mode");
        let md = self.metadata(path)?;
        if md.is_symlink() {
            return Err(FsError::new(FsOp::SetMode, path, FsErrorKind::Symlink));
        }
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|e| FsError::from_io(FsOp::SetMode, path, e))
    }

    fn free_space(&self, path: &Path) -> FsResult<u64> {
        ui_thread::assert_not_ui_thread("Fs::free_space");
        let st = rustix::fs::statvfs(path)
            .map_err(|e| FsError::from_io(FsOp::FreeSpace, path, io::Error::from(e)))?;
        Ok(st.f_bavail.saturating_mul(st.f_frsize))
    }

    fn reflink_supported(&self, dir_a: &Path, dir_b: &Path) -> FsResult<bool> {
        ui_thread::assert_not_ui_thread("Fs::reflink_supported");
        reflink_probe(dir_a, dir_b).map_err(|e| FsError::from_io(FsOp::Reflink, dir_a, e))
    }

    fn copy_file(&self, from: &Path, to: &Path, mode: u32) -> FsResult<bool> {
        ui_thread::assert_not_ui_thread("Fs::copy_file");
        let mut src = OpenOptions::new()
            .read(true)
            .custom_flags(o_nofollow())
            .open(from)
            .map_err(|e| FsError::from_io(FsOp::Read, from, e))?;
        let md = src
            .metadata()
            .map_err(|e| FsError::from_io(FsOp::Read, from, e))?;
        if !md.is_file() {
            return Err(FsError::new(FsOp::Read, from, FsErrorKind::InvalidInput));
        }
        let mut dst = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(o_nofollow())
            .open(to)
            .map_err(|e| FsError::from_io(FsOp::Write, to, e))?;
        let result = (|| -> io::Result<bool> {
            let cloned = rustix::fs::ioctl_ficlone(&dst, &src).is_ok();
            if !cloned {
                io::copy(&mut src, &mut dst)?;
            }
            dst.set_permissions(fs::Permissions::from_mode(mode & 0o7777))?;
            Ok(cloned)
        })();
        match result {
            Ok(cloned) => Ok(cloned),
            Err(e) => {
                drop(dst);
                let _ = fs::remove_file(to);
                Err(FsError::from_io(FsOp::Write, to, e))
            }
        }
    }
}
