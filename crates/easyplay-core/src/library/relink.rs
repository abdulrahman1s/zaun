//! Moving an executable (LOGIC_SPECS §8): verify the new file, compute the new root and
//! every dependent path, commit in one transaction. The prefix, private home and overlay
//! are keyed by `GameId` and never move.

use std::path::{Component, Path, PathBuf};

use super::no_game;
use super::service::{Library, storage};
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::Fs;
use crate::identity::matcher::same_identity;
use crate::identity::pe::PeLimits;
use crate::identity::{SharedLocations, fingerprint, game_root};
use crate::ids::{ExecutableId, GameId};
use crate::model::{
    Executable, Fingerprint, Game, GameDirAccess, PeInfo, SaveDataLocation, WorkingDir,
};
use crate::paths::normalize;
use crate::storage::{ExecutableRepo, GamePatch, GameRepo};

/// How the new file relates to the stored one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verdict {
    /// Equal quick hash: the same file, moved.
    SameContent,
    /// Different content, same PE identity: probably an update ("a different version").
    NewVersion,
    /// Different content and identity: probably another program (stronger wording,
    /// default Cancel).
    DifferentProgram,
}

/// The Verify step.
pub fn verify(stored: &Executable, fp: &Fingerprint, pe: &PeInfo) -> Verdict {
    if stored.fingerprint.quick_hash == fp.quick_hash {
        Verdict::SameContent
    } else if same_identity(&stored.pe, pe) {
        Verdict::NewVersion
    } else {
        Verdict::DifferentProgram
    }
}

/// Result of [`Library::relink`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelinkOutcome {
    /// Committed.
    Relinked {
        /// The game.
        game: GameId,
        /// Its new root.
        new_root: Option<PathBuf>,
        /// What the new file was.
        verdict: Verdict,
    },
    /// The content differs; ask the user, then call again with `confirmed = true`.
    NeedsConfirmation {
        /// The game.
        game: GameId,
        /// Wording to use.
        verdict: Verdict,
    },
}

/// The new file.
#[derive(Clone, Debug)]
pub struct NewLocation {
    /// As chosen.
    pub path: PathBuf,
    /// Canonical.
    pub canonical: PathBuf,
    /// Mount point.
    pub mount_point: Option<PathBuf>,
    /// Fingerprint.
    pub fingerprint: Fingerprint,
    /// PE information.
    pub pe: PeInfo,
}

/// `path` minus a trailing `tail` (component-wise), when `path` ends with it.
fn strip_tail(path: &Path, tail: &Path) -> Option<PathBuf> {
    let p: Vec<Component<'_>> = path.components().collect();
    let t: Vec<Component<'_>> = tail.components().collect();
    if t.is_empty() || t.len() >= p.len() || p[p.len() - t.len()..] != t[..] {
        return None;
    }
    Some(p[..p.len() - t.len()].iter().collect())
}

/// Move `path` from inside `old_root` to the same relative place inside `new_root`.
fn moved(path: &Path, old_root: &Path, new_root: &Path) -> Option<PathBuf> {
    let p = normalize(path);
    let rel = p.strip_prefix(normalize(old_root)).ok()?;
    Some(new_root.join(rel))
}

/// The Compute step: the game with every path rewritten. Validation of the result
/// (sensitive directories, cross-field rules) is the caller's.
pub fn compute(
    fs: &dyn Fs,
    shared: &SharedLocations,
    game: &Game,
    exe: ExecutableId,
    to: &NewLocation,
) -> Result<Game> {
    let stored = game
        .executables
        .iter()
        .find(|e| e.id == exe)
        .ok_or_else(|| no_game(game.id))?;
    let old_root = game.root.clone();
    let rel = stored.rel_path.clone().or_else(|| {
        old_root
            .as_ref()
            .and_then(|r| stored.canonical_path.strip_prefix(r).ok())
            .map(Path::to_path_buf)
    });
    let tail_root = match (&old_root, &rel) {
        (Some(_), Some(rel)) => strip_tail(&to.canonical, rel)
            .filter(|r| shared.classify(r).is_none() && !shared.is_mount_root(r)),
        _ => None,
    };
    let new_root = match tail_root {
        Some(r) => Some(r),
        None => game_root::detect(fs, shared, &to.canonical)?.root,
    };
    let mut g = game.clone();
    for e in &mut g.executables {
        if e.id == exe {
            e.path = to.path.clone();
            e.canonical_path = to.canonical.clone();
            e.fingerprint = to.fingerprint;
            e.pe = to.pe.clone();
            e.mount_point = to.mount_point.clone();
            e.rel_path = new_root
                .as_ref()
                .and_then(|r| to.canonical.strip_prefix(r).ok())
                .map(Path::to_path_buf);
        } else if let (Some(nr), Some(r)) = (&new_root, e.rel_path.clone()) {
            // Kept even if the file is missing at the new place.
            e.path = nr.join(&r);
            e.canonical_path = nr.join(&r);
            e.mount_point = to.mount_point.clone();
        }
    }
    if let (Some(or), Some(nr)) = (&old_root, &new_root) {
        for f in &mut g.policy.folders {
            if let Some(c) = moved(&f.canonical, or, nr) {
                f.path = c.clone();
                f.canonical = c;
            }
        }
        if let SaveDataLocation::External { dir } = &mut g.policy.save_data
            && let Some(d) = moved(dir, or, nr)
        {
            *dir = d;
        }
        if let WorkingDir::Custom(p) = &mut g.launch.working_dir
            && let Some(d) = moved(p, or, nr)
        {
            *p = d;
        }
    }
    // Loose-file rule.
    match (&new_root, g.policy.game_dir) {
        (None, _) => g.policy.game_dir = GameDirAccess::FileOnly,
        (Some(_), GameDirAccess::FileOnly) => g.policy.game_dir = GameDirAccess::ReadWrite,
        _ => {}
    }
    g.root = new_root;
    Ok(g)
}

impl Library {
    /// Re-link `exe` to `new_path` (manual "Locate…", a `MovedCandidate` match, or a unique
    /// candidate). A content mismatch needs `confirmed`.
    pub fn relink(
        &mut self,
        exe: ExecutableId,
        new_path: &Path,
        confirmed: bool,
    ) -> Result<RelinkOutcome> {
        let (game_id, stored) = self
            .db
            .executable(exe)
            .map_err(storage)?
            .ok_or_else(|| Error::new(ErrorCode::LibExeNotFound).with("executable", exe))?;
        let fs = self.deps.fs.clone();
        let canonical = fs.canonicalize(new_path)?;
        if let Some((owner, other)) = self
            .db
            .executable_by_canonical_path(&canonical)
            .map_err(storage)?
            && other.id != exe
        {
            return Err(Error::new(ErrorCode::LibAlreadyInLibrary).with("game", owner));
        }
        let fp = fingerprint::compute(&*fs, &canonical)?;
        let pe = self
            .deps
            .pe
            .inspect(&canonical, &PeLimits::default())
            .unwrap_or_else(|_| PeInfo::unknown());
        let verdict = verify(&stored, &fp, &pe);
        if verdict != Verdict::SameContent && !confirmed {
            return Ok(RelinkOutcome::NeedsConfirmation {
                game: game_id,
                verdict,
            });
        }
        let old = self.game(game_id)?;
        let to = NewLocation {
            path: new_path.to_path_buf(),
            mount_point: self
                .deps
                .mounts
                .mount_point_of(&canonical)
                .map(|m| m.mount_point.clone()),
            canonical,
            fingerprint: fp,
            pe,
        };
        let new = compute(&*fs, &self.deps.shared, &old, exe, &to)?;
        self.validate_game(&new, Some(&old))?;
        let now = self.deps.clock.now();
        let patch = GamePatch {
            root: Some(new.root.clone()),
            policy: (new.policy != old.policy).then(|| new.policy.clone()),
            launch: (new.launch != old.launch).then(|| new.launch.clone()),
            ..GamePatch::default()
        };
        let changed: Vec<Executable> = new
            .executables
            .iter()
            .filter(|e| old.executables.iter().all(|o| o != *e))
            .cloned()
            .collect();
        self.db.transaction(|tx| -> Result<()> {
            for e in &changed {
                tx.update_executable(e).map_err(storage)?;
            }
            tx.update_game(game_id, &patch, now).map_err(storage)?;
            Ok(())
        })?;
        self.refresh_game(game_id)?;
        Ok(RelinkOutcome::Relinked {
            game: game_id,
            new_root: new.root,
            verdict,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails() {
        assert_eq!(
            strip_tail(
                Path::new("/new/Game/bin/game.exe"),
                Path::new("bin/game.exe")
            ),
            Some(PathBuf::from("/new/Game"))
        );
        assert_eq!(
            strip_tail(Path::new("/x/game.exe"), Path::new("bin/game.exe")),
            None
        );
        assert_eq!(
            strip_tail(Path::new("/game.exe"), Path::new("game.exe")),
            Some(PathBuf::from("/"))
        );
    }
}
