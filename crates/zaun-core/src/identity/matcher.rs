//! Which game a file is: the decision procedure of GAME_IDENTITY §4.
//!
//! Pure: the library is seen through [`LibraryIndex`] and the filesystem through
//! [`PathProbe`], so every rule is table-testable.

use std::path::{Path, PathBuf};

use crate::clock::Timestamp;
use crate::ids::{ExecutableId, GameId};
use crate::model::{Fingerprint, PeInfo, Sha256};

/// One executable as the index knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedExe {
    /// Its game.
    pub game: GameId,
    /// The executable.
    pub exe: ExecutableId,
    /// Stored canonical path.
    pub canonical_path: PathBuf,
    /// Stored mount point.
    pub mount_point: Option<PathBuf>,
    /// Stored fingerprint.
    pub fingerprint: Fingerprint,
    /// Stored PE information.
    pub pe: PeInfo,
    /// When the game was last played (orders copy candidates).
    pub last_played_at: Option<Timestamp>,
}

/// The library index of ADR-0008 "Consequences".
pub trait LibraryIndex {
    /// The executable whose canonical path is `path`.
    fn by_path(&self, path: &Path) -> Option<IndexedExe>;
    /// Executables with this quick hash.
    fn by_hash(&self, hash: &Sha256) -> Vec<IndexedExe>;
    /// Game roots.
    fn roots(&self) -> Vec<(PathBuf, GameId)>;
}

/// Filesystem facts the procedure needs.
pub trait PathProbe {
    /// Whether something exists at `path`.
    fn exists(&self, path: &Path) -> bool;
    /// Whether `mount_point` is currently mounted.
    fn is_mounted(&self, mount_point: &Path) -> bool;
}

/// Rule 2 vs rule 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Change {
    /// Same identity strings: an update; refresh silently.
    SameIdentity,
    /// Different identity: ask.
    IdentityDiffers,
}

/// Hint attached to [`MatchOutcome::Unknown`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnknownHint {
    /// Several missing executables have this content (rule 7).
    AmbiguousMove {
        /// The candidates.
        candidates: Vec<(GameId, ExecutableId)>,
    },
}

/// Outcome of [`resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchOutcome {
    /// Rule 1: same path, same content.
    Known {
        /// Game.
        game: GameId,
        /// Executable.
        exe: ExecutableId,
    },
    /// Rules 2 and 3: same path, new content.
    KnownChanged {
        /// Game.
        game: GameId,
        /// Executable.
        exe: ExecutableId,
        /// Whether the identity strings still agree.
        change: Change,
    },
    /// Rule 4: the content of a missing executable reappeared here.
    MovedCandidate {
        /// Game.
        game: GameId,
        /// Executable to re-link.
        exe: ExecutableId,
    },
    /// Rule 5: a copy of an executable that still exists.
    CopyCandidate {
        /// The most recently played game among the candidates.
        game: GameId,
        /// Its executable with this content.
        exe: ExecutableId,
    },
    /// Rule 6: inside the root of a game (deepest root wins).
    InsideKnownRoot {
        /// Game.
        game: GameId,
    },
    /// Rules 7 and 8.
    Unknown {
        /// Why the dialog should say more.
        hint: Option<UnknownHint>,
    },
}

fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `same_identity` of §4: every one of `product_name`, `company`, `original_filename` that is
/// present in both is equal after normalisation, and at least one is present in both.
pub fn same_identity(a: &PeInfo, b: &PeInfo) -> bool {
    let pairs = [
        (&a.product_name, &b.product_name),
        (&a.company, &b.company),
        (&a.original_filename, &b.original_filename),
    ];
    let mut compared = 0;
    for (x, y) in pairs {
        let (Some(x), Some(y)) = (x, y) else { continue };
        let (x, y) = (norm(x), norm(y));
        if x.is_empty() || y.is_empty() {
            continue;
        }
        if x != y {
            return false;
        }
        compared += 1;
    }
    compared > 0
}

/// Whether the stored path of `e` is gone *and* its drive is connected (a disconnected
/// drive is never evidence of a move).
fn is_gone(e: &IndexedExe, probe: &dyn PathProbe) -> bool {
    if probe.exists(&e.canonical_path) {
        return false;
    }
    match &e.mount_point {
        Some(m) if m != Path::new("/") => probe.is_mounted(m),
        _ => true,
    }
}

/// Resolve the file at canonical path `path` with fresh fingerprint `fp` and PE info `pe`.
pub fn resolve(
    path: &Path,
    fp: &Fingerprint,
    pe: &PeInfo,
    index: &dyn LibraryIndex,
    probe: &dyn PathProbe,
) -> MatchOutcome {
    // Rules 1–3.
    if let Some(e) = index.by_path(path) {
        if e.fingerprint.quick_hash == fp.quick_hash {
            return MatchOutcome::Known {
                game: e.game,
                exe: e.exe,
            };
        }
        let change = if same_identity(&e.pe, pe) {
            Change::SameIdentity
        } else {
            Change::IdentityDiffers
        };
        return MatchOutcome::KnownChanged {
            game: e.game,
            exe: e.exe,
            change,
        };
    }
    // Content matches, minus those a full hash proves different.
    let same: Vec<IndexedExe> = index
        .by_hash(&fp.quick_hash)
        .into_iter()
        .filter(|e| e.canonical_path != path)
        .filter(|e| match (e.fingerprint.full_hash, fp.full_hash) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        })
        .collect();
    let (gone, present): (Vec<_>, Vec<_>) = same.into_iter().partition(|e| is_gone(e, probe));
    // Rule 4.
    if let [e] = gone.as_slice() {
        return MatchOutcome::MovedCandidate {
            game: e.game,
            exe: e.exe,
        };
    }
    // Rule 5.
    if let Some(e) = present
        .iter()
        .max_by_key(|e| (e.last_played_at, std::cmp::Reverse(e.exe)))
    {
        return MatchOutcome::CopyCandidate {
            game: e.game,
            exe: e.exe,
        };
    }
    // Rule 6.
    if let Some((_, game)) = index
        .roots()
        .into_iter()
        .filter(|(r, _)| path.starts_with(r))
        .max_by_key(|(r, _)| r.components().count())
    {
        return MatchOutcome::InsideKnownRoot { game };
    }
    // Rules 7 and 8.
    if gone.len() > 1 {
        return MatchOutcome::Unknown {
            hint: Some(UnknownHint::AmbiguousMove {
                candidates: gone.iter().map(|e| (e.game, e.exe)).collect(),
            }),
        };
    }
    MatchOutcome::Unknown { hint: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pe(product: Option<&str>, company: Option<&str>, orig: Option<&str>) -> PeInfo {
        PeInfo {
            product_name: product.map(Into::into),
            company: company.map(Into::into),
            original_filename: orig.map(Into::into),
            ..PeInfo::default()
        }
    }

    #[test]
    fn identity_rules() {
        let a = pe(Some("Hollow  Knight"), Some("Team Cherry"), None);
        let b = pe(Some("hollow knight"), None, Some("hk.exe"));
        assert!(same_identity(&a, &b));
        assert!(!same_identity(&a, &pe(Some("Other"), None, None)));
        assert!(!same_identity(&PeInfo::default(), &PeInfo::default()));
        assert!(!same_identity(&a, &pe(None, None, Some("hk.exe"))));
        assert!(!same_identity(
            &pe(Some("X"), Some("A"), None),
            &pe(Some("X"), Some("B"), None)
        ));
    }
}
