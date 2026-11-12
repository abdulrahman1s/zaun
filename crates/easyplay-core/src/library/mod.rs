//! Library use cases and the in-memory index.
//!
//! Layer 5. See MODULES §6, GAME_LIBRARY, GAME_IDENTITY and LOGIC_SPECS §1–2, §8–13.
//!
//! | File | Content |
//! |---|---|
//! | [`index`] | [`LibrarySnapshot`], summaries, search keys, overlays, the matcher's [`ExeIndex`] |
//! | [`query`] | collections, filters, search operators, sorting, grouping (pure; UI thread) |
//! | [`draft`] | [`GameDraft`] for an unknown executable (LOGIC_SPECS §2 steps 1–7) |
//! | [`service`] | [`Library`]: create, update, tags, favourites, hidden, executables, orphans |
//! | [`missing`] | availability classification with per-mount deadlines (§9) |
//! | [`relink`] | moving a game to a new location (§8) |
//! | [`remove`] | detach / collect / purge (§11) |
//! | [`prefix_ops`] | prefix reset, drive-letter links, health heuristics, sizes (§12) |
//! | [`programs`] | a game's programs: role, label, arguments, removal, detaching |
//! | [`installer`] | programs installed by an installer session (§13) |
//! | [`export`] | exported game settings as TOML (CONFIGURATION §4) |
//! | [`folder_scan`] | programs in a dropped folder (GAME_LIBRARY §11) |
//! | [`resolve`] | `<GAME>` resolution of the command line (CONFIGURATION §6) |
//!
//! Seams left for later layers: the launch pipeline and the AppService call into
//! [`Library`]; the library itself never launches anything. Host-dependent policy
//! validation (`sandbox::validate_policy` needs `HostFacts`) enters through
//! [`service::PolicyCheck`], runtime labels and version order through
//! [`index::RuntimeInfo`], the umu-database title match through [`draft::UmuTitleMatch`],
//! and desktop shortcut removal through [`remove::RemoveHooks`].

pub mod draft;
pub mod export;
pub mod folder_scan;
pub mod index;
pub mod installer;
pub mod missing;
pub mod prefix_ops;
pub mod programs;
pub mod query;
pub mod relink;
pub mod remove;
pub mod resolve;
pub mod service;

pub use draft::{DraftEnv, GameDraft, build_draft};
pub use folder_scan::{FolderScan, FoundProgram};
pub use index::{
    DefaultRuntimeInfo, ExeIndex, LibrarySnapshot, LibraryTotals, Overlays, RuntimeInfo,
    SearchEntry, SearchIndex, TagInfo, normalize,
};
pub use installer::InstalledProgram;
pub use missing::{ScanTarget, ScanThrottle, ScanTrigger};
pub use query::{
    Collection, Filters, GroupSpan, Grouping, LibraryQuery, PresetFilter, QueryResult, SortKey,
    run_query,
};
pub use relink::{RelinkOutcome, Verdict};
pub use remove::{RemoveFiles, RemoveJob, RemoveOptions, RemoveReport};
pub use service::{Library, LibraryDeps, LibraryService, PolicyCheck};

pub use prefix_ops::ResetOptions;

use crate::error::{Error, ErrorCode};
use crate::model::{EntryKind, GameFlags, Problem, ValidationError};

/// `games.flags` bit owned by the library: the entry is an installer that has not been
/// converted yet (LOGIC_SPECS §13). `model::GameFlags` preserves unknown bits, so the bit
/// survives every round trip; it should move into `model::GameFlags` with the next model
/// change.
pub const INSTALLER_ENTRY: GameFlags = GameFlags::from_bits_retain(1 << 8);
/// `games.flags` bit owned by the library: a tool added as its own entry.
pub const TOOL_ENTRY: GameFlags = GameFlags::from_bits_retain(1 << 9);

/// The entry kind encoded in a game's flags.
pub fn entry_kind(flags: GameFlags) -> EntryKind {
    if flags.contains(INSTALLER_ENTRY) {
        EntryKind::Installer
    } else if flags.contains(TOOL_ENTRY) {
        EntryKind::Tool
    } else {
        EntryKind::Game
    }
}

/// `flags` with the entry-kind bits set for `kind`.
pub fn with_entry_kind(flags: GameFlags, kind: EntryKind) -> GameFlags {
    let base = flags.difference(INSTALLER_ENTRY | TOOL_ENTRY);
    match kind {
        EntryKind::Game => base,
        EntryKind::Installer => base | INSTALLER_ENTRY,
        EntryKind::Tool => base | TOOL_ENTRY,
    }
}

/// `EP-LIB-030` for a field.
pub(crate) fn invalid(field: &'static str, problem: Problem) -> Error {
    Error::from(ValidationError::new(field, problem))
}

/// `EP-LIB-030` for a count limit of GAME_LIBRARY §13.
pub(crate) fn limit(field: &'static str, max: usize) -> Error {
    invalid(field, Problem::TooMany { max })
}

/// An unknown game (`EP-INT-001` with the game in the context).
pub(crate) fn no_game(id: crate::ids::GameId) -> Error {
    Error::new(ErrorCode::IntTaskFailed)
        .with("game", id)
        .with("reason", "no such game")
}
