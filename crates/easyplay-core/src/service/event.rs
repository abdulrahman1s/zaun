//! What the AppService tells the UI (ARCHITECTURE §6). Snapshot changes are announced, not
//! carried: the new value is in the [`super::SnapshotHub`].
//!
//! User-visible text is not produced here. Errors arrive as `UserError` (written by
//! `error::catalog`); notices arrive as typed [`Notice`]s that the UI words.

use std::path::PathBuf;

use super::command::OpenOrigin;
use crate::error::UserError;
use crate::ids::{GameId, LaunchId, QueryId, SessionId};
use crate::launch::LaunchState;
use crate::library::GameDraft;
use crate::model::{Game, PromptRequest, SlrVariant};
use crate::stats::StatsResult;

/// The context of a setup dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupContext {
    /// Where the file came from.
    pub origin: OpenOrigin,
    /// Further drafts waiting behind this one.
    pub queued: usize,
}

/// A short, typed notice for a toast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A game was added.
    GameAdded {
        /// The game.
        game: GameId,
        /// Its name.
        name: String,
    },
    /// The opened file belongs to a game already in the library.
    AlreadyInLibrary {
        /// The game.
        game: GameId,
    },
    /// A moved game was found in its new location.
    FoundMoved {
        /// The game.
        game: GameId,
    },
    /// More than 64 paths were opened; the rest were ignored.
    OnlyFirstPaths {
        /// How many were processed.
        processed: usize,
    },
    /// A game was removed.
    GameRemoved {
        /// Its name.
        name: String,
        /// Where its save data was kept.
        saved_data: Option<PathBuf>,
    },
    /// The game's icon could not be stored; it keeps the generated cover.
    ArtworkKeptPlaceholder {
        /// The game.
        game: GameId,
    },
    /// Artwork was fetched from the Steam CDN.
    ArtworkFetched {
        /// The game.
        game: GameId,
        /// Slots stored.
        slots: usize,
    },
    /// A runtime variant was copied into the store.
    RuntimeAdopted {
        /// The variant.
        variant: SlrVariant,
        /// Reflinked (no extra space used).
        reflinked: bool,
    },
    /// The private Windows storage was reset.
    PrefixReset {
        /// The game.
        game: GameId,
    },
    /// A stop was requested.
    StopRequested {
        /// The session.
        session: SessionId,
    },
    /// Play history was deleted (`None` = all games).
    StatisticsCleared {
        /// The game.
        game: Option<GameId>,
        /// Sessions deleted.
        sessions: u64,
    },
    /// One session was removed from history.
    SessionRemoved {
        /// The session.
        session: SessionId,
    },
    /// A program was added to a game.
    ProgramAdded {
        /// The game.
        game: GameId,
    },
    /// A program was found at its new place.
    ProgramLocated {
        /// The game.
        game: GameId,
    },
    /// A tool started in the game's private storage.
    ToolStarted {
        /// The game.
        game: GameId,
    },
    /// A launch was cancelled before the game started.
    LaunchCancelled {
        /// The game.
        game: GameId,
    },
    /// The diagnostics report was saved.
    ReportSaved {
        /// Where.
        path: PathBuf,
    },
    /// The damaged library was resolved (repaired, restored or replaced).
    LibraryRecovered {
        /// What was done.
        how: RecoveredHow,
        /// History rows that could not be recovered, when known.
        history_rows_lost: Option<u64>,
    },
}

/// How a damaged library was resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveredHow {
    /// Salvage recovered every game (automatic).
    Repaired,
    /// A backup was restored.
    Restored,
    /// The partly repaired library is in use.
    UsedRepaired,
    /// An empty library was started.
    StartedEmpty,
}

/// A chunk of a session log file (`Command::ReadLog`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogChunk {
    /// The session.
    pub session: SessionId,
    /// The file.
    pub file: super::command::LogFile,
    /// Offset of `bytes`.
    pub offset: u64,
    /// Content (sanitising is the viewer's job).
    pub bytes: Vec<u8>,
    /// File size when read.
    pub size: u64,
    /// The file does not exist (logs removed).
    pub missing: bool,
}

/// The answer to a query command.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryResult {
    /// `Command::LoadGame`.
    Game(Box<Game>),
    /// `Command::QueryStats`.
    Stats(StatsResult),
    /// `Command::ScanFolder`.
    Folder(crate::library::FolderScan),
    /// `Command::MeasurePrefix`: bytes (and whether the walk visited everything).
    PrefixSize {
        /// Bytes of regular files.
        bytes: u64,
        /// The walk was complete.
        complete: bool,
    },
    /// `Command::Inspect`.
    Inspector(Box<crate::launch::InspectorData>),
    /// `Command::ListSessions`.
    Sessions(Vec<crate::model::Session>),
    /// `Command::ReadLog`.
    LogChunk(LogChunk),
    /// `Command::ListProcesses`.
    Processes(Vec<crate::session::ProcessInfo>),
    /// `Command::BuildReport`.
    Report(Vec<crate::diagnostics::report::ReportFile>),
    /// The query failed.
    Failed(Box<UserError>),
}

/// An announcement from the AppService.
#[derive(Clone, Debug)]
pub enum Event {
    /// A new library snapshot is in the hub.
    LibraryChanged,
    /// A new open-sessions snapshot is in the hub.
    SessionsChanged,
    /// A new runtime catalogue is in the hub.
    RuntimesChanged,
    /// New preferences are in the hub.
    PreferencesChanged,
    /// New host facts are in the hub.
    HostFactsChanged,
    /// The diagnostics snapshot (Doctor report, crash record, recovery mode) changed.
    DiagnosticsChanged,
    /// A launch advanced.
    LaunchProgress {
        /// The game.
        game: GameId,
        /// The launch.
        launch: LaunchId,
        /// The state entered.
        state: LaunchState,
    },
    /// An unknown program needs the setup dialog.
    SetupRequested {
        /// The proposal.
        draft: Box<GameDraft>,
        /// Context.
        context: SetupContext,
    },
    /// A flow needs a decision.
    Prompt(PromptRequest),
    /// The UI should show this game's page (a known program was dropped or picked).
    ShowGame(GameId),
    /// A query answered (`LoadGame`, `QueryStats`).
    QueryResult {
        /// The request's ID.
        id: QueryId,
        /// Result.
        result: QueryResult,
    },
    /// A short notice.
    Notice(Notice),
    /// An error not attached to a launch.
    Failure(Box<UserError>),
    /// The service stopped (after `Shutdown`).
    QuitRequested,
}
