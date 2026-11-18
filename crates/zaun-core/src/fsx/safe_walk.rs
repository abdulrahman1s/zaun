//! Bounded directory walks that never follow symlinks and stop at mount boundaries, and
//! deletion confined to a root and one filesystem.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{FileType, Fs, FsError, FsErrorKind, FsOp, FsResult, Metadata, is_within};
use crate::clock::{Clock, MonoInstant};

/// Bounds of a walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkLimits {
    /// Maximum number of entries visited.
    pub max_entries: usize,
    /// Maximum depth (children of the root have depth 1).
    pub max_depth: usize,
    /// Monotonic deadline.
    pub deadline: Option<MonoInstant>,
}

impl Default for WalkLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_depth: 32,
            deadline: None,
        }
    }
}

impl WalkLimits {
    /// Limits with a deadline `budget` from now.
    pub fn with_budget(clock: &dyn Clock, budget: Duration) -> Self {
        Self {
            deadline: Some(clock.mono().saturating_add(budget)),
            ..Self::default()
        }
    }
}

/// One visited entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkEntry {
    /// Full path.
    pub path: PathBuf,
    /// Depth below the root (1 = direct child).
    pub depth: usize,
    /// Metadata of the entry itself (`lstat`).
    pub metadata: Metadata,
    /// A directory on another filesystem than the root; it is reported but not entered.
    pub crosses_mount: bool,
}

/// What the visitor wants next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkControl {
    /// Keep going (enter this entry if it is a directory).
    Continue,
    /// Do not enter this directory.
    SkipDir,
    /// End the walk.
    Stop,
}

/// Why a walk ended early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkStop {
    /// `max_entries` reached.
    MaxEntries,
    /// The deadline passed.
    Deadline,
    /// The visitor returned [`WalkControl::Stop`].
    Visitor,
}

/// Outcome of a walk.
#[derive(Debug, Default)]
pub struct WalkSummary {
    /// Entries visited.
    pub entries: usize,
    /// Why the walk ended early, if it did.
    pub stopped: Option<WalkStop>,
    /// Directories not entered because of `max_depth`.
    pub depth_limited: usize,
    /// Directories not entered because they are on another filesystem.
    pub mount_boundaries: usize,
    /// Directories that could not be listed.
    pub errors: Vec<FsError>,
}

/// Walk the tree below `root` depth-first in name order, calling `visit` for every entry.
/// Symlinks are reported but never followed; directories on another device than `root` are
/// reported with `crosses_mount` and not entered. `root` itself must be a directory and not a
/// symlink.
pub fn safe_walk(
    fs: &dyn Fs,
    root: &Path,
    limits: WalkLimits,
    clock: &dyn Clock,
    mut visit: impl FnMut(&WalkEntry) -> WalkControl,
) -> FsResult<WalkSummary> {
    let root_md = fs.metadata(root)?;
    if root_md.is_symlink() {
        return Err(FsError::new(FsOp::Walk, root, FsErrorKind::Symlink));
    }
    if !root_md.is_dir() {
        return Err(FsError::new(FsOp::Walk, root, FsErrorKind::NotADirectory));
    }
    let mut summary = WalkSummary::default();
    // Stack of (directory, depth of its children); children pushed in reverse for name order.
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_owned(), 1)];
    while let Some((dir, depth)) = stack.pop() {
        let entries = match fs.read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                summary.errors.push(e);
                continue;
            }
        };
        let mut subdirs = Vec::new();
        for entry in entries {
            if summary.entries >= limits.max_entries {
                summary.stopped = Some(WalkStop::MaxEntries);
                return Ok(summary);
            }
            if limits.deadline.is_some_and(|d| clock.mono() >= d) {
                summary.stopped = Some(WalkStop::Deadline);
                return Ok(summary);
            }
            let md = match fs.metadata(&entry.path) {
                Ok(md) => md,
                Err(e) => {
                    summary.errors.push(e);
                    continue;
                }
            };
            let crosses_mount = md.is_dir() && md.device != root_md.device;
            let walk_entry = WalkEntry {
                path: entry.path.clone(),
                depth,
                metadata: md,
                crosses_mount,
            };
            summary.entries += 1;
            match visit(&walk_entry) {
                WalkControl::Stop => {
                    summary.stopped = Some(WalkStop::Visitor);
                    return Ok(summary);
                }
                WalkControl::SkipDir => {}
                WalkControl::Continue => {
                    if md.file_type == FileType::Dir {
                        if crosses_mount {
                            summary.mount_boundaries += 1;
                        } else if depth >= limits.max_depth {
                            summary.depth_limited += 1;
                        } else {
                            subdirs.push(entry.path);
                        }
                    }
                }
            }
        }
        for sub in subdirs.into_iter().rev() {
            stack.push((sub, depth + 1));
        }
    }
    Ok(summary)
}

/// What [`safe_remove_tree`] did.
#[derive(Debug, Default)]
pub struct RemoveReport {
    /// Files and symlinks removed.
    pub files_removed: usize,
    /// Directories removed.
    pub dirs_removed: usize,
    /// Paths left in place because they are on another filesystem.
    pub skipped_mounts: Vec<PathBuf>,
    /// Failures (the tree is then incomplete; callers retry at next start, `ZN-FS-004`).
    pub errors: Vec<FsError>,
}

impl RemoveReport {
    /// Whether everything was removed.
    pub fn is_complete(&self) -> bool {
        self.skipped_mounts.is_empty() && self.errors.is_empty()
    }
}

/// Delete `target` and everything below it, confined to `root`:
///
/// - `target` must lie inside `root` (component-wise), and no component between `root` and
///   `target` may be a symlink;
/// - symlinks are unlinked, never followed;
/// - directories on another filesystem than `target` are left in place (reported);
/// - a missing `target` is not an error.
///
/// Fails only when the preconditions do not hold; individual failures are in the report.
pub fn safe_remove_tree(fs: &dyn Fs, root: &Path, target: &Path) -> FsResult<RemoveReport> {
    let root = crate::paths::normalize(root);
    let target = crate::paths::normalize(target);
    if !is_within(&root, &target) || target == root {
        return Err(FsError::new(FsOp::Walk, &target, FsErrorKind::OutsideRoot));
    }
    // Every intermediate component must be a real directory.
    let rel = target.strip_prefix(&root).unwrap_or(&target);
    let mut cur = root.clone();
    let comps: Vec<_> = rel.components().collect();
    for comp in &comps[..comps.len().saturating_sub(1)] {
        cur.push(comp);
        match fs.metadata(&cur) {
            Ok(md) if md.is_dir() => {}
            Ok(md) if md.is_symlink() => {
                return Err(FsError::new(FsOp::Walk, &cur, FsErrorKind::Symlink));
            }
            Ok(_) => return Err(FsError::new(FsOp::Walk, &cur, FsErrorKind::NotADirectory)),
            Err(e) if e.kind == FsErrorKind::NotFound => return Ok(RemoveReport::default()),
            Err(e) => return Err(e),
        }
    }
    let mut report = RemoveReport::default();
    let md = match fs.metadata(&target) {
        Ok(md) => md,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(report),
        Err(e) => return Err(e),
    };
    if md.is_dir() {
        remove_dir_recursive(fs, &target, md.device, &mut report);
    } else {
        match fs.remove_file(&target) {
            Ok(()) => report.files_removed += 1,
            Err(e) => report.errors.push(e),
        }
    }
    Ok(report)
}

fn remove_dir_recursive(fs: &dyn Fs, dir: &Path, device: u64, report: &mut RemoveReport) {
    let entries = match fs.read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            report.errors.push(e);
            return;
        }
    };
    let mut blocked = false;
    for entry in entries {
        let md = match fs.metadata(&entry.path) {
            Ok(md) => md,
            Err(e) => {
                report.errors.push(e);
                blocked = true;
                continue;
            }
        };
        if md.is_dir() {
            if md.device != device {
                report.skipped_mounts.push(entry.path);
                blocked = true;
                continue;
            }
            let before = report.skipped_mounts.len() + report.errors.len();
            remove_dir_recursive(fs, &entry.path, device, report);
            if report.skipped_mounts.len() + report.errors.len() != before {
                blocked = true;
            }
        } else {
            match fs.remove_file(&entry.path) {
                Ok(()) => report.files_removed += 1,
                Err(e) => {
                    report.errors.push(e);
                    blocked = true;
                }
            }
        }
    }
    if !blocked {
        match fs.remove_dir(dir) {
            Ok(()) => report.dirs_removed += 1,
            Err(e) => report.errors.push(e),
        }
    }
}
