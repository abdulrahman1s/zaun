//! Game root detection (GAME_IDENTITY §5): the smallest directory that holds everything the
//! game needs and nothing personal.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::clock::Clock;
use crate::fsx::{Fs, FsResult, WalkControl, WalkLimits, safe_walk};

use super::sensitive::{SharedLocations, SharedReason};

/// Directory names that hold binaries rather than being the game (§5).
pub const BINARY_DIR_NAMES: &[&str] = &[
    "bin", "bin32", "bin64", "binaries", "x64", "x86", "x86_64", "win32", "win64", "system",
    "system32", "game", "retail", "release", "shipping", "exe",
];
/// Architecture directory names climbed only below a `Binaries` directory.
pub const ARCH_DIR_NAMES: &[&str] = &["win64", "win32", "wingdk", "x64"];
/// Most directories climbed above the executable's folder.
const MAX_CLIMB: usize = 4;

/// Whether `name` (any case) is in [`BINARY_DIR_NAMES`].
pub fn is_binary_dir_name(name: &str) -> bool {
    let l = name.to_lowercase();
    BINARY_DIR_NAMES.contains(&l.as_str())
}

/// Folder or loose file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RootKind {
    /// The game has a folder.
    Folder,
    /// The executable is in a shared location: file-only access.
    LooseFile,
}

/// Why the proposal is what it is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RootReason {
    /// The executable's own folder.
    ExecutableFolder,
    /// Climbed out of binary directories (`bin`, `Binaries/Win64`, …).
    BinaryDirs,
    /// Unreal layout: `<Root>/<Project>/Binaries/<Arch>` with `<Root>/Engine`.
    UnrealLayout,
    /// The executable's folder is shared.
    Shared(SharedReason),
}

/// Result of [`detect`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRootProposal {
    /// Folder or loose file.
    pub kind: RootKind,
    /// The proposed root (`None` for a loose file).
    pub root: Option<PathBuf>,
    /// Every acceptable choice, from the executable's folder upwards; `root` is the last.
    pub candidates: Vec<PathBuf>,
    /// Why.
    pub reason: RootReason,
}

fn base_lower(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// `game_root::detect` of §5. Reads only names of directories on the path upwards plus one
/// `lstat` of `<parent>/Engine` for the Unreal rule; never lists contents.
pub fn detect(fs: &dyn Fs, shared: &SharedLocations, exe: &Path) -> FsResult<GameRootProposal> {
    let exe = fs.canonicalize(exe)?;
    let Some(parent) = exe.parent().map(Path::to_path_buf) else {
        return Ok(GameRootProposal {
            kind: RootKind::LooseFile,
            root: None,
            candidates: Vec::new(),
            reason: RootReason::Shared(SharedReason::MountRoot),
        });
    };
    if let Some(reason) = shared.classify(&parent) {
        return Ok(GameRootProposal {
            kind: RootKind::LooseFile,
            root: None,
            candidates: Vec::new(),
            reason: RootReason::Shared(reason),
        });
    }
    let mut candidates = vec![parent.clone()];
    let mut dir = parent;
    let mut climbed_binaries = false;
    for _ in 0..MAX_CLIMB {
        let name = base_lower(&dir);
        let up = match dir.parent() {
            Some(p) => p.to_path_buf(),
            None => break,
        };
        let arch_under_binaries =
            ARCH_DIR_NAMES.contains(&name.as_str()) && base_lower(&up) == "binaries";
        if !(BINARY_DIR_NAMES.contains(&name.as_str()) || arch_under_binaries) {
            break;
        }
        if shared.is_shared_location(&up) || shared.is_mount_root(&up) {
            break;
        }
        climbed_binaries = name == "binaries";
        candidates.push(up.clone());
        dir = up;
    }
    let mut reason = if candidates.len() > 1 {
        RootReason::BinaryDirs
    } else {
        RootReason::ExecutableFolder
    };
    // Unreal: <Root>/<Project>/Binaries/Win64/Game.exe with <Root>/Engine present.
    if climbed_binaries
        && let Some(last) = candidates.last().cloned()
        && let Some(up) = last.parent()
        && fs.metadata(&up.join("Engine")).is_ok_and(|m| m.is_dir())
        && !shared.is_shared_location(up)
        && !shared.is_mount_root(up)
    {
        candidates.push(up.to_path_buf());
        reason = RootReason::UnrealLayout;
    }
    Ok(GameRootProposal {
        kind: RootKind::Folder,
        root: candidates.last().cloned(),
        candidates,
        reason,
    })
}

/// Why a user-chosen root was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootChoiceError {
    /// The executable is not inside the chosen folder.
    NotAnAncestor,
    /// The chosen folder is shared.
    Shared(SharedReason),
}

/// Re-validate a root chosen in the setup dialog ("Choose folder…"): an ancestor-or-self of
/// the executable's folder that is not shared. Paths are canonical.
pub fn check_choice(
    shared: &SharedLocations,
    exe: &Path,
    chosen: &Path,
) -> Result<(), RootChoiceError> {
    let Some(parent) = exe.parent() else {
        return Err(RootChoiceError::NotAnAncestor);
    };
    if !parent.starts_with(chosen) {
        return Err(RootChoiceError::NotAnAncestor);
    }
    match shared.classify(chosen) {
        Some(r) => Err(RootChoiceError::Shared(r)),
        None => Ok(()),
    }
}

/// Entries counted at most for the breadth warning.
pub const BREADTH_MAX_ENTRIES: usize = 10_001;

/// The bounded entry count of a proposed root for the breadth warning (§5): depth 2, at most
/// [`BREADTH_MAX_ENTRIES`], 500 ms. Returns `(count, complete)`.
pub fn breadth(fs: &dyn Fs, clock: &dyn Clock, root: &Path) -> FsResult<(usize, bool)> {
    let limits = WalkLimits {
        max_entries: BREADTH_MAX_ENTRIES,
        max_depth: 2,
        ..WalkLimits::with_budget(clock, Duration::from_millis(500))
    };
    let summary = safe_walk(fs, root, limits, clock, |_| WalkControl::Continue)?;
    Ok((summary.entries, summary.stopped.is_none()))
}
