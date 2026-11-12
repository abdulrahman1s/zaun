//! Removing a game (LOGIC_SPECS §11): Detach (soft delete), Collect (save data kept, private
//! storage and artwork deleted with `fsx::safe_remove_tree`), Purge (rows). Nothing outside
//! `AppPaths::game_dir(id)`, `artwork/<id>/` and the desktop shortcuts is touched; the game
//! folder and an external save folder never are.

use std::path::PathBuf;

use super::service::{Library, storage};
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{Fs, FsErrorKind, safe_remove_tree};
use crate::ids::GameId;
use crate::model::{Game, SaveDataLocation};
use crate::paths::AppPaths;
use crate::storage::GameRepo;

/// Options of the removal dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoveOptions {
    /// Delete the prefix and private home (default true).
    pub delete_private_storage: bool,
    /// Move the Windows profile out first (default true when saves are internal).
    pub keep_save_data: bool,
    /// Remove menu and desktop shortcuts (default true).
    pub delete_shortcuts: bool,
    /// Remove the overlay of a Protected game folder (default true).
    pub delete_overlay: bool,
    /// `stats.keep_history_of_removed`.
    pub keep_history: bool,
}

impl Default for RemoveOptions {
    fn default() -> Self {
        Self {
            delete_private_storage: true,
            keep_save_data: true,
            delete_shortcuts: true,
            delete_overlay: true,
            keep_history: true,
        }
    }
}

/// What a removal did.
#[derive(Debug, Default)]
pub struct RemoveReport {
    /// The removed game's name (toast).
    pub name: String,
    /// Where the save data was kept, if it was.
    pub saved_data: Option<PathBuf>,
    /// Whether every file was removed; otherwise the row stays soft-deleted and the start-up
    /// retry finishes the job.
    pub complete: bool,
    /// Failures of the Collect step.
    pub errors: Vec<Error>,
}

/// Side effects owned by other modules (desktop shortcuts and exported icons).
pub trait RemoveHooks {
    /// Remove the game's shortcuts; errors are reported, not fatal.
    fn remove_shortcuts(&self, game: GameId) -> Result<()>;
}

/// No shortcuts to remove.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHooks;

impl RemoveHooks for NoHooks {
    fn remove_shortcuts(&self, _game: GameId) -> Result<()> {
        Ok(())
    }
}

/// `<data>/saved-data/<sanitised name>-<short id>`.
pub fn saved_data_dir(paths: &AppPaths, game: GameId, name: &str) -> PathBuf {
    let mut clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    clean = clean.trim_matches('_').chars().take(48).collect();
    if clean.is_empty() {
        clean.push_str("game");
    }
    paths
        .saved_data_dir
        .join(format!("{clean}-{}", game.short()))
}

/// The Windows profile inside the prefix.
pub fn profile_dir(paths: &AppPaths, game: GameId) -> PathBuf {
    paths
        .prefix_dir(game)
        .join("drive_c")
        .join("users")
        .join("steamuser")
}

/// Move the profile out of the prefix (same-filesystem rename of the directory itself; a
/// symlink in its place is not followed and not kept). `Ok(None)` when there is nothing to
/// keep.
pub fn keep_save_data(
    fs: &dyn Fs,
    paths: &AppPaths,
    game: GameId,
    name: &str,
) -> Result<Option<PathBuf>> {
    let from = profile_dir(paths, game);
    match fs.metadata(&from) {
        Ok(md) if md.is_dir() => {}
        Ok(_) => return Ok(None),
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    // Every component between the game directory and the profile must be a real directory.
    let mut cur = paths.prefix_dir(game);
    for part in ["", "drive_c", "users"] {
        if !part.is_empty() {
            cur.push(part);
        }
        let md = fs.metadata(&cur)?;
        if !md.is_dir() {
            return Err(Error::from(crate::fsx::FsError::new(
                crate::fsx::FsOp::Rename,
                &cur,
                FsErrorKind::Symlink,
            )));
        }
    }
    fs.create_dir_all(&paths.saved_data_dir, 0o700)?;
    let mut to = saved_data_dir(paths, game, name);
    let mut n = 1;
    while fs.exists(&to) {
        n += 1;
        to = PathBuf::from(format!(
            "{}-{n}",
            saved_data_dir(paths, game, name).display()
        ));
    }
    fs.rename(&from, &to)?;
    Ok(Some(to))
}

/// The Collect step without the save-data move.
fn collect(
    fs: &dyn Fs,
    paths: &AppPaths,
    game: GameId,
    opts: &RemoveOptions,
    hooks: &dyn RemoveHooks,
    errors: &mut Vec<Error>,
) {
    let mut remove =
        |root: &std::path::Path, target: PathBuf| match safe_remove_tree(fs, root, &target) {
            Ok(r) => {
                errors.extend(r.errors.into_iter().map(Error::from).chain(
                    r.skipped_mounts.into_iter().map(|p| {
                        Error::new(ErrorCode::FsDeleteIncomplete).with("path", p.display())
                    }),
                ))
            }
            Err(e) => errors.push(e.into()),
        };
    if opts.delete_private_storage {
        remove(&paths.games_dir, paths.game_dir(game));
    } else if opts.delete_overlay {
        remove(&paths.games_dir, paths.overlay_dir(game));
    }
    remove(&paths.artwork_dir, paths.artwork_game_dir(game));
    if opts.delete_shortcuts
        && let Err(e) = hooks.remove_shortcuts(game)
    {
        errors.push(e);
    }
}

/// A detached game whose files are still to be collected (step 2 runs on the job pool).
#[derive(Clone, Debug)]
pub struct RemoveJob {
    /// The game.
    pub id: GameId,
    /// Its name.
    pub name: String,
    /// The dialog options.
    pub opts: RemoveOptions,
    /// Save data lives inside the prefix.
    pub internal_saves: bool,
}

/// What step 2 did on disk.
#[derive(Debug)]
pub struct RemoveFiles {
    /// Where the save data went; an error aborts before anything is deleted.
    pub saved_data: Result<Option<PathBuf>>,
    /// Deletions that failed.
    pub errors: Vec<Error>,
}

impl RemoveJob {
    /// Removal, step 2 (any thread): keep the save data, then delete the private storage,
    /// the artwork and the shortcuts.
    pub fn run(&self, fs: &dyn Fs, paths: &AppPaths, hooks: &dyn RemoveHooks) -> RemoveFiles {
        let saved_data = if self.opts.keep_save_data && self.internal_saves {
            keep_save_data(fs, paths, self.id, &self.name)
        } else {
            Ok(None)
        };
        let mut errors = Vec::new();
        if saved_data.is_ok() {
            collect(fs, paths, self.id, &self.opts, hooks, &mut errors);
        }
        RemoveFiles { saved_data, errors }
    }
}

impl Library {
    /// Remove a game. The caller has stopped it (LOGIC_SPECS §11 precondition) and holds the
    /// flow lock.
    pub fn remove(
        &mut self,
        id: GameId,
        opts: RemoveOptions,
        hooks: &dyn RemoveHooks,
    ) -> Result<RemoveReport> {
        let job = self.begin_remove(id, opts)?;
        let files = job.run(&*self.deps.fs.clone(), &self.deps.paths.clone(), hooks);
        self.finish_remove(&job, files)
    }

    /// Removal, step 1 (service thread): detach the game (it leaves the library at once).
    pub fn begin_remove(&mut self, id: GameId, opts: RemoveOptions) -> Result<RemoveJob> {
        let game: Game = self.game(id)?;
        let now = self.deps.clock.now();
        self.db.set_game_deleted(id, Some(now)).map_err(storage)?;
        self.refresh_game(id)?;
        Ok(RemoveJob {
            id,
            name: game.name,
            opts,
            internal_saves: game.policy.save_data == SaveDataLocation::Internal,
        })
    }

    /// Removal, step 3 (service thread): purge the rows when the files are gone, restore
    /// the game when its save data could not be kept, or leave the soft-deleted row for the
    /// start-up retry.
    pub fn finish_remove(&mut self, job: &RemoveJob, files: RemoveFiles) -> Result<RemoveReport> {
        let id = job.id;
        let mut report = RemoveReport {
            name: job.name.clone(),
            ..RemoveReport::default()
        };
        match files.saved_data {
            Ok(kept) => report.saved_data = kept,
            Err(e) => {
                self.db.set_game_deleted(id, None).map_err(storage)?;
                self.refresh_game(id)?;
                return Err(e);
            }
        }
        report.errors = files.errors;
        if !report.errors.is_empty() {
            tracing::warn!(game = %id, errors = report.errors.len(), "removal incomplete; retried at next start");
            return Ok(report);
        }
        // Purge.
        self.db
            .purge_game(id, job.opts.keep_history)
            .map_err(storage)?;
        self.exes.remove_game(id);
        report.complete = true;
        Ok(report)
    }

    /// Start-up retry for soft-deleted rows (idempotent). Save data still inside the prefix
    /// is kept first, whatever the original choice was: the safe default after a crash.
    pub fn retry_removals(
        &mut self,
        keep_history: bool,
        hooks: &dyn RemoveHooks,
    ) -> Result<Vec<GameId>> {
        let fs = self.deps.fs.clone();
        let paths = self.deps.paths.clone();
        let mut done = Vec::new();
        for id in self.db.deleted_games().map_err(storage)? {
            let name = self
                .db
                .game_record(id)
                .map_err(storage)?
                .map(|r| r.name)
                .unwrap_or_default();
            if let Err(e) = keep_save_data(&*fs, &paths, id, &name) {
                tracing::warn!(game = %id, code = %e.code, "keeping save data before retry failed");
                continue;
            }
            let mut errors = Vec::new();
            collect(
                &*fs,
                &paths,
                id,
                &RemoveOptions::default(),
                hooks,
                &mut errors,
            );
            if errors.is_empty() {
                self.db.purge_game(id, keep_history).map_err(storage)?;
                self.exes.remove_game(id);
                done.push(id);
            }
        }
        Ok(done)
    }
}
