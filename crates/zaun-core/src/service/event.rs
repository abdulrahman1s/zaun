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
    pub origin: OpenOrigin,
    /// Further drafts waiting behind this one.
    pub queued: usize,
}

/// A short, typed notice for a toast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A game was added.
    GameAdded { game: GameId, name: String },
    /// The opened file belongs to a game already in the library.
    AlreadyInLibrary { game: GameId },
    /// A moved game was found in its new location.
    FoundMoved { game: GameId },
    /// More than 64 paths were opened; the rest were ignored.
    OnlyFirstPaths { processed: usize },
    /// A game was removed.
    GameRemoved {
        name: String,
        /// Where its save data was kept.
        saved_data: Option<PathBuf>,
    },
    /// The game's icon could not be stored; it keeps the generated cover.
    ArtworkKeptPlaceholder { game: GameId },
    /// Artwork was fetched from the Steam CDN.
    ArtworkFetched { game: GameId, slots: usize },
    /// A runtime variant was copied into the store.
    RuntimeAdopted {
        variant: SlrVariant,
        /// Reflinked (no extra space used).
        reflinked: bool,
    },
    /// The private Windows storage was reset.
    PrefixReset { game: GameId },
    /// A stop was requested.
    StopRequested { session: SessionId },
    /// Play history was deleted (`None` = all games).
    StatisticsCleared { game: Option<GameId>, sessions: u64 },
    /// One session was removed from history.
    SessionRemoved { session: SessionId },
    /// A program was added to a game.
    ProgramAdded { game: GameId },
    /// A program was found at its new place.
    ProgramLocated { game: GameId },
    /// A tool started in the game's private storage.
    ToolStarted { game: GameId },
    /// A launch was cancelled before the game started.
    LaunchCancelled { game: GameId },
    /// The diagnostics report was saved.
    ReportSaved { path: PathBuf },
    /// A request was handed to Steam (ADR-0031).
    SteamAsked {
        /// The game (`None` for the downloads page of every game).
        game: Option<GameId>,
        /// What was asked.
        request: crate::model::SteamRequest,
    },
    /// Steam's files were read on request.
    SteamScanned { games: usize },
    /// New messages: received, or sent by the user (ADR-0033). Oldest first.
    ChatMessages(Vec<crate::model::ChatMessage>),
    /// Messages whose delivery or text changed.
    ChatMessagesUpdated(Vec<crate::model::ChatMessage>),
    /// `ChatCommand::OpenLink` with `chat.check_links` on: the link was not opened, the
    /// user decides (`ChatCommand::OpenLinkAnyway`). `unchecked`: the check got no answer
    /// (offline, or its format changed); otherwise Google Safe Browsing flagged the site.
    ChatUnsafeLink {
        /// The link as written.
        url: String,
        /// The host that was checked.
        host: String,
        /// No answer (rather than a warning).
        unchecked: bool,
    },
    /// `ChatCommand::SendGif` failed: the GIF could not be downloaded, is too large to send,
    /// or the saved file is gone.
    ChatGifNotSent,
    /// `ChatCommand::SaveFile` finished: the copy is in `folder` (the folder's name).
    ChatFileSaved {
        /// The folder's name.
        folder: String,
    },
    /// A Steam game's launch options were written to Steam's settings (ADR-0031 §6a).
    SteamLaunchOptionsSaved { game: GameId },
    /// The damaged library was resolved (repaired, restored or replaced).
    LibraryRecovered {
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
    pub session: SessionId,
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
    /// `Command::AboutGame`.
    About(Box<crate::identity::GameAbout>),
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
    /// `Command::FindSteamApp`.
    SteamApps {
        /// The app ID the computer gives (stored, a file, the umu-database).
        detected: Option<(u32, crate::artwork::steam::SteamAppIdSource)>,
        /// Store search results, closest name first.
        candidates: Vec<crate::artwork::steam::SearchCandidate>,
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
    /// `SteamCommand::Details`.
    Steam(Box<SteamDetail>),
    /// `ChatCommand::LoadChat`.
    Chat(ChatPage),
    /// `ChatCommand::GifSearch` and `GifTrending`.
    Gifs(crate::gifs::GifAnswer),
    /// `ChatCommand::GifPreview`.
    GifPreview(crate::gifs::GifPreview),
    /// `ChatCommand::LoadPicture`: a picture file for the crop step (ADR-0038).
    PictureSource(crate::model::PictureSource),
    /// The query failed.
    Failed(Box<UserError>),
}

/// What a Steam game's page shows beyond its summary (ADR-0031).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamDetail {
    pub game: GameId,
    /// What Steam says (the summary's facts).
    pub facts: std::sync::Arc<crate::model::SteamFacts>,
    /// Notes kept by Zaun.
    pub notes: String,
    /// Steam runs the game now.
    pub running: bool,
    /// The program that reaches Steam is installed (Play, Update in Steam, Open in Steam).
    pub client_found: bool,
    /// The Windows-storage actions are possible now: Proton, a prefix, the Proton build
    /// that last ran it present, the game not running.
    pub prefix_tools: bool,
    /// The game's launch options as Steam has them (`Some("")`: none set); `None` when the
    /// account's settings file cannot be found or read (ADR-0031 §6a).
    pub launch_options: Option<String>,
    /// A Steam client runs now: the launch options cannot be changed until it is closed.
    pub steam_open: bool,
}

/// The answer to `ChatCommand::LoadChat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatPage {
    pub chat: crate::model::ChatRef,
    /// Oldest first.
    pub messages: Vec<crate::model::ChatMessage>,
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
    /// A new Chat snapshot is in the hub (ADR-0033).
    ChatChanged,
    /// A launch advanced.
    LaunchProgress {
        game: GameId,
        launch: LaunchId,
        state: LaunchState,
    },
    /// An unknown program needs the setup dialog.
    SetupRequested {
        draft: Box<GameDraft>,
        context: SetupContext,
    },
    /// A flow needs a decision.
    Prompt(PromptRequest),
    /// The UI should show this game's page (a known program was dropped or picked).
    ShowGame(GameId),
    /// A query answered (`LoadGame`, `QueryStats`).
    QueryResult { id: QueryId, result: QueryResult },
    /// A short notice.
    Notice(Notice),
    /// An error not attached to a launch.
    Failure(Box<UserError>),
    /// The service stopped (after `Shutdown`).
    QuitRequested,
}
