//! What Zaun can tell about a game from its files (ADR-0028): the engine it is made with
//! and what its main program's folder reveals (anti-cheat, Steam, store SDKs). Read when the
//! game page asks; informational only.

use std::path::Path;

use crate::clock::Clock;
use crate::fsx::Fs;
use crate::model::{ExeRole, Game};

use super::engine::{EngineGuess, EngineInput, detect};
use super::hints::{ExeHints, exe_hints, list_names};

/// What a game's files tell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameAbout {
    /// The engine, when the files tell.
    pub engine: Option<EngineGuess>,
    /// Anti-cheat, Steam and store findings beside the main program.
    pub hints: ExeHints,
}

/// Read what `game`'s files tell: a listing of its main program's folder, of the game
/// folder, and of the few folders the engine rules look into, each bounded as
/// [`list_names`] is. A game without a main program tells nothing.
pub fn about(fs: &dyn Fs, clock: &dyn Clock, game: &Game) -> GameAbout {
    let Some(exe) = game.executables.iter().find(|e| e.role == ExeRole::Primary) else {
        return GameAbout::default();
    };
    let Some(dir) = exe.path.parent() else {
        return GameAbout::default();
    };
    let siblings = list_names(fs, clock, dir);
    let root = game.root.as_deref();
    let root_names = match root {
        Some(r) if r != dir => list_names(fs, clock, r),
        _ => Vec::new(),
    };
    let exe_dirs: Vec<String> = dir
        .ancestors()
        .take_while(|a| root.is_none_or(|r| a.starts_with(r)))
        .filter_map(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    let sub = |name: &str| -> Vec<String> {
        if siblings.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            list_names(fs, clock, &dir.join(name))
        } else {
            Vec::new()
        }
    };
    let exe_name = exe
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let engine = detect(&EngineInput {
        exe_name: &exe_name,
        exe_dirs: &exe_dirs,
        pe: &exe.pe,
        siblings: &siblings,
        root: &root_names,
        sub: &sub,
    });
    GameAbout {
        engine,
        hints: exe_hints(&exe.pe, &siblings),
    }
}
