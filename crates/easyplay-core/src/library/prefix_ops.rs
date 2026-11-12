//! Prefix operations (LIB-009): "Reset private Windows storage" with stash/restore and
//! rollback (LOGIC_SPECS §12), drive-letter links in `<prefix>/dosdevices` for folder grants
//! (BUBBLEWRAP_SANDBOX §4.11, V-006), the corrupt-prefix heuristics of ERROR_HANDLING §4.6,
//! and the size of a game's private storage.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::remove::profile_dir;
use super::service::{Library, storage};
use crate::clock::Clock;
use crate::error::{Error, Result};
use crate::fsx::{
    FileType, Fs, FsError, FsErrorKind, FsOp, WalkControl, WalkLimits, safe_remove_tree, safe_walk,
};
use crate::ids::GameId;
use crate::jobs::CancelToken;
use crate::model::{FolderGrant, GameFlags, SaveDataLocation};
use crate::paths::AppPaths;
use crate::storage::{GamePatch, GameRepo};

/// Options of the reset dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetOptions {
    /// Keep the Windows profile (default true).
    pub keep_save_data: bool,
    /// Reset the private home too (default true).
    pub also_reset_home: bool,
}

impl Default for ResetOptions {
    fn default() -> Self {
        Self {
            keep_save_data: true,
            also_reset_home: true,
        }
    }
}

/// What a reset did on disk.
#[derive(Debug, Default)]
pub struct ResetReport {
    /// The profile was stashed and restored.
    pub kept_save_data: bool,
    /// Trash directories that could not be deleted yet (retried by the sweep).
    pub leftover_trash: Vec<PathBuf>,
}

/// One rename performed, for rollback.
struct Done {
    from: PathBuf,
    to: PathBuf,
}

fn rollback(fs: &dyn Fs, done: &mut Vec<Done>) {
    while let Some(d) = done.pop() {
        if let Err(e) = fs.rename(&d.to, &d.from) {
            tracing::error!(error = %e, "prefix reset rollback failed");
        }
    }
}

fn real_dir(fs: &dyn Fs, p: &Path) -> Result<bool> {
    match fs.metadata(p) {
        Ok(md) if md.is_dir() => Ok(true),
        Ok(md) if md.is_symlink() => {
            Err(FsError::new(FsOp::Rename, p, FsErrorKind::Symlink).into())
        }
        Ok(_) => Err(FsError::new(FsOp::Rename, p, FsErrorKind::NotADirectory).into()),
        Err(e) if e.kind == FsErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// The filesystem part of the reset. Every step is a same-directory rename; a failure
/// undoes the renames done so far and returns the error (nothing deleted). The `.trash-*`
/// directories are deleted last, best effort.
pub fn reset_files(
    fs: &dyn Fs,
    paths: &AppPaths,
    id: GameId,
    opts: ResetOptions,
    internal_saves: bool,
    stamp: i64,
) -> Result<ResetReport> {
    let game_dir = paths.game_dir(id);
    let prefix = paths.prefix_dir(id);
    let home = paths.home_dir(id);
    let stash = game_dir.join(".stash-users");
    let mut done: Vec<Done> = Vec::new();
    let mut report = ResetReport::default();
    let step = |done: &mut Vec<Done>, from: &Path, to: &Path| -> Result<()> {
        fs.rename(from, to)?;
        done.push(Done {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
        Ok(())
    };
    let result = (|| -> Result<()> {
        // Stash.
        let profile = profile_dir(paths, id);
        let stashed = opts.keep_save_data && internal_saves && {
            // Components of the profile path must be real directories (no symlink following).
            real_dir(fs, &prefix)?
                && real_dir(fs, &prefix.join("drive_c"))?
                && real_dir(fs, &prefix.join("drive_c/users"))?
                && real_dir(fs, &profile)?
        };
        if stashed {
            step(&mut done, &profile, &stash)?;
        }
        // Remove.
        let trash_prefix = game_dir.join(format!(".trash-prefix-{stamp}"));
        if real_dir(fs, &prefix)? {
            step(&mut done, &prefix, &trash_prefix)?;
        }
        let trash_home = game_dir.join(format!(".trash-home-{stamp}"));
        if opts.also_reset_home && real_dir(fs, &home)? {
            step(&mut done, &home, &trash_home)?;
        }
        fs.create_dir(&prefix, 0o700)?;
        if opts.also_reset_home {
            match fs.create_dir(&home, 0o700) {
                Ok(()) => {}
                Err(e) if e.kind == FsErrorKind::AlreadyExists => {}
                Err(e) => {
                    let _ = fs.remove_dir(&prefix);
                    return Err(e.into());
                }
            }
        }
        if stashed {
            let users = prefix.join("drive_c").join("users");
            let restored = fs
                .create_dir_all(&users, 0o700)
                .map_err(Error::from)
                .and_then(|()| fs.rename(&stash, &profile).map_err(Error::from));
            if let Err(e) = restored {
                let _ = safe_remove_tree(fs, &game_dir, &prefix);
                if opts.also_reset_home {
                    let _ = fs.remove_dir(&home);
                }
                return Err(e);
            }
            report.kept_save_data = true;
        }
        Ok(())
    })();
    if let Err(e) = result {
        rollback(fs, &mut done);
        return Err(e);
    }
    for d in done.iter().filter(|d| {
        d.to.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".trash-"))
    }) {
        match safe_remove_tree(fs, &game_dir, &d.to) {
            Ok(r) if r.is_complete() => {}
            _ => report.leftover_trash.push(d.to.clone()),
        }
    }
    Ok(report)
}

impl Library {
    /// "Reset private Windows storage" (LOGIC_SPECS §12). The caller checked the game is
    /// not running and holds the flow lock.
    pub fn reset_prefix(&mut self, id: GameId, opts: ResetOptions) -> Result<ResetReport> {
        let game = self.game(id)?;
        let now = self.deps.clock.now();
        let internal = game.policy.save_data == SaveDataLocation::Internal;
        let report = reset_files(
            &*self.deps.fs,
            &self.deps.paths,
            id,
            opts,
            internal,
            now.as_millis(),
        )?;
        let patch = GamePatch {
            prefix_prepared_with: Some(None),
            flags: Some(game.flags.difference(GameFlags::SUSPECTED_TAMPER)),
            ..GamePatch::default()
        };
        self.db.transaction(|tx| -> Result<()> {
            tx.set_prefix_tainted_at(id, None).map_err(storage)?;
            tx.update_game(id, &patch, now).map_err(storage)?;
            Ok(())
        })?;
        self.refresh_game(id)?;
        Ok(report)
    }
}

// ---- Drive letters -------------------------------------------------------------------------

/// What [`sync_drive_letters`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DriveReport {
    /// Links created or replaced.
    pub created: Vec<char>,
    /// Stale links removed.
    pub removed: Vec<char>,
}

/// Make `<prefix>/dosdevices/<l>:` a symlink to the grant's canonical path for every grant
/// with a drive letter, and remove other `d:`…`y:` symlinks (they belong to grants that
/// were removed). Only symlinks are ever removed; nothing is followed.
pub fn sync_drive_letters(
    fs: &dyn Fs,
    prefix: &Path,
    grants: &[FolderGrant],
) -> Result<DriveReport> {
    let dos = prefix.join("dosdevices");
    if !real_dir(fs, &dos)? {
        fs.create_dir_all(&dos, 0o700)?;
    }
    let mut report = DriveReport::default();
    let wanted: Vec<(char, &Path)> = grants
        .iter()
        .filter_map(|g| {
            g.drive_letter
                .map(|l| (l.to_ascii_lowercase(), g.canonical.as_path()))
        })
        .filter(|(l, _)| crate::model::is_valid_drive_letter(*l))
        .collect();
    for l in 'd'..='y' {
        let link = dos.join(format!("{l}:"));
        let current = match fs.metadata(&link) {
            Ok(md) => Some(md.file_type),
            Err(e) if e.kind == FsErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        match (wanted.iter().find(|(w, _)| *w == l), current) {
            (Some((_, target)), Some(FileType::Symlink)) => {
                if fs.read_link(&link)? != *target {
                    fs.remove_file(&link)?;
                    fs.symlink(target, &link)?;
                    report.created.push(l);
                }
            }
            (Some((_, target)), None) => {
                fs.symlink(target, &link)?;
                report.created.push(l);
            }
            (Some(_), Some(_)) => {
                // A real file or directory where the link should be: leave it, report.
                return Err(FsError::new(FsOp::Symlink, &link, FsErrorKind::AlreadyExists).into());
            }
            (None, Some(FileType::Symlink)) => {
                fs.remove_file(&link)?;
                report.removed.push(l);
            }
            (None, _) => {}
        }
    }
    Ok(report)
}

// ---- Health (ERROR_HANDLING §4.6) ------------------------------------------------------

/// A failed health check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrefixProblem {
    /// `drive_c` missing while the prefix is marked prepared.
    DriveCMissing,
    /// A registry file is missing.
    RegistryMissing(&'static str),
    /// A registry file is empty.
    RegistryEmpty(&'static str),
    /// A registry file lacks the `WINE REGISTRY` header.
    RegistryHeader(&'static str),
    /// Prepare failed twice in a row.
    PrepareFailedTwice,
}

/// Registry files checked.
pub const REGISTRY_FILES: [&str; 2] = ["system.reg", "user.reg"];

/// The file-based heuristics. `prepared` = `games.prefix_prepared_with` is set; an
/// unprepared prefix is not judged.
pub fn prefix_health(
    fs: &dyn Fs,
    prefix: &Path,
    prepared: bool,
    prepare_failures: u32,
) -> Vec<PrefixProblem> {
    let mut out = Vec::new();
    if prepare_failures >= 2 {
        out.push(PrefixProblem::PrepareFailedTwice);
    }
    if !prepared {
        return out;
    }
    if !fs
        .metadata(&prefix.join("drive_c"))
        .is_ok_and(|m| m.is_dir())
    {
        out.push(PrefixProblem::DriveCMissing);
    }
    for f in REGISTRY_FILES {
        let p = prefix.join(f);
        match fs.metadata(&p) {
            Ok(md) if md.is_file() && md.size == 0 => out.push(PrefixProblem::RegistryEmpty(f)),
            Ok(md) if md.is_file() => {
                let head = fs.read_range(&p, 0, 64).unwrap_or_default();
                if !head.starts_with(b"WINE REGISTRY") {
                    out.push(PrefixProblem::RegistryHeader(f));
                }
            }
            _ => out.push(PrefixProblem::RegistryMissing(f)),
        }
    }
    out
}

/// Log lines that indicate a damaged prefix.
pub fn log_indicates_corruption(line: &str) -> bool {
    line.contains("could not load kernel32.dll")
        || (line.contains("wineserver") && line.contains("corrupt"))
        || line.contains("Bad EXE format")
}

// ---- Size ----------------------------------------------------------------------------------

/// Size of a tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SizeReport {
    /// Bytes of regular files.
    pub bytes: u64,
    /// Regular files.
    pub files: u64,
    /// Whether the walk visited everything (no limit, deadline or cancellation hit).
    pub complete: bool,
}

/// Size of `root` (job; bounded, no symlink following, stays on one filesystem).
pub fn tree_size(
    fs: &dyn Fs,
    clock: &dyn Clock,
    root: &Path,
    budget: Duration,
    cancel: &CancelToken,
) -> Result<SizeReport> {
    let mut r = SizeReport::default();
    let limits = WalkLimits {
        max_entries: 2_000_000,
        max_depth: 64,
        ..WalkLimits::with_budget(clock, budget)
    };
    let summary = match safe_walk(fs, root, limits, clock, |e| {
        if cancel.is_cancelled() {
            return WalkControl::Stop;
        }
        if e.metadata.is_file() {
            r.bytes = r.bytes.saturating_add(e.metadata.size);
            r.files += 1;
        }
        WalkControl::Continue
    }) {
        Ok(s) => s,
        Err(e) if e.kind == FsErrorKind::NotFound => {
            r.complete = true;
            return Ok(r);
        }
        Err(e) => return Err(e.into()),
    };
    r.complete = summary.stopped.is_none() && summary.errors.is_empty();
    Ok(r)
}
