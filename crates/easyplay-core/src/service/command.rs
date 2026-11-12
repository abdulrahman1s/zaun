//! What the UI, the instance socket and the CLI ask the AppService to do (ARCHITECTURE §6).
//!
//! A command either succeeds silently (the change arrives as a snapshot) or produces
//! `Event::Failure` / `LaunchProgress { state: Failed(..) }`. There are no synchronous
//! replies. Commands not implemented by this build answer with `EP-INT-003`.

use std::path::PathBuf;

use crate::artwork::ArtSource;
use crate::config::PreferencePatch;
use crate::ids::{ExecutableId, GameId, PromptId, QueryId, RuntimeName, SessionId, TagName};
use crate::library::{GameDraft, RemoveOptions, ResetOptions};
use crate::model::{ArtSlot, LaunchConfig, PromptAnswer, RuntimeRef, SandboxPolicy, SlrVariant};
use crate::stats::StatsRequest;

pub use crate::session::StopMode;

/// Where opened paths come from (LOGIC_SPECS §1, origin-specific behaviour).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpenOrigin {
    /// "Open with EasyPlay" in a file manager.
    FileManager,
    /// Dropped on the window.
    DragDrop,
    /// The add-game file picker.
    Picker,
    /// `easyplay open` / `easyplay add`.
    Cli,
}

impl OpenOrigin {
    /// Whether a known program is started (file manager, CLI) rather than shown (drop,
    /// picker: "dropping a file into the library reads as add, not run").
    pub const fn launches_known(self) -> bool {
        matches!(self, OpenOrigin::FileManager | OpenOrigin::Cli)
    }
}

/// What follows a successful `CreateGame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AfterCreate {
    /// Only add ("Add").
    Nothing,
    /// Add, then launch ("Play").
    Launch,
}

/// Where a launch was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LaunchOrigin {
    /// The library or the game page.
    Library,
    /// A file opened from the file manager (quick-launch layout).
    QuickLaunch,
    /// `easyplay launch` forwarded to the running instance.
    Cli,
}

/// A command for the AppService.
/// One change to a game, as plain data (the UI's game page). Mapped onto
/// `storage::GamePatch` by the service; several edits apply together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameEdit {
    /// Rename (1–80 characters, user-set).
    Name(String),
    /// Notes text.
    Notes(String),
    /// The whole sandbox policy.
    Policy(Box<SandboxPolicy>),
    /// The whole launch configuration.
    Launch(Box<LaunchConfig>),
    /// Compatibility layer and whether to follow its channel.
    Runtime {
        /// Reference.
        reference: RuntimeRef,
        /// Follow updates.
        follow: bool,
    },
    /// Game-fix entry (`None` = no fix).
    UmuId(Option<String>),
    /// User-set Steam app ID for artwork (`None` clears it).
    SteamAppId(Option<u32>),
    /// Switch to a preset, keeping folder grants, game-folder access and save location
    /// (`sandbox::apply_preset` with the current host facts).
    Preset(crate::model::BasePreset),
    /// Back to the name derived from the primary program.
    ResetName,
    /// Entry kind (game, installer, tool).
    Kind(crate::model::EntryKind),
    /// A program's role; `Primary` demotes the current primary to `Alternate`.
    ExeRole(ExecutableId, crate::model::ExeRole),
    /// A program's label (`None` clears it).
    ExeLabel(ExecutableId, Option<String>),
    /// A program's own arguments (`None` = the game's arguments).
    ExeArgs(ExecutableId, Option<Vec<String>>),
    /// Remove a program (not the primary).
    RemoveExe(ExecutableId),
}

/// A folder to show in the file manager (`Command::OpenFolder`). The service resolves it to
/// a location it knows; nothing else can be opened (DESKTOP_INTEGRATION §8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderRequest {
    /// The game folder (the program's folder for a single file).
    GameFolder(GameId),
    /// The save data (external folder, or the profile inside the prefix).
    SaveData(GameId),
    /// The private Windows storage.
    Prefix(GameId),
    /// A session's log folder.
    Session(SessionId),
    /// EasyPlay's data folder.
    DataFolder,
    /// A folder granted to some game (refused otherwise).
    Granted(PathBuf),
}

#[derive(Clone, Debug)]
pub enum Command {
    // ---- entry points
    /// Open up to 64 paths (LOGIC_SPECS §1).
    OpenPaths {
        /// The paths, in order.
        paths: Vec<PathBuf>,
        /// Where they come from.
        origin: OpenOrigin,
    },
    // ---- library
    /// Add a game from a confirmed setup draft (LOGIC_SPECS §2).
    CreateGame {
        /// The draft as edited in the setup dialog.
        draft: Box<GameDraft>,
        /// Launch afterwards.
        then: AfterCreate,
    },
    /// The setup dialog was closed without adding (the next queued draft is shown).
    DiscardDraft,
    /// Remove a game (LOGIC_SPECS §11).
    RemoveGame {
        /// The game.
        id: GameId,
        /// Dialog options.
        options: RemoveOptions,
    },
    /// Favourite on/off (idempotent).
    SetFavorite {
        /// The game.
        id: GameId,
        /// New value.
        favorite: bool,
    },
    /// Replace a game's tags.
    SetTags {
        /// The game.
        id: GameId,
        /// The tags (unknown ones are created).
        tags: Vec<TagName>,
    },
    /// Hidden on/off.
    SetHidden {
        /// The game.
        id: GameId,
        /// New value.
        hidden: bool,
    },
    /// Change a game (validated as a whole; `Event::Failure` on a refused change).
    UpdateGame {
        /// Game.
        id: GameId,
        /// Changes.
        edits: Vec<GameEdit>,
    },
    /// Load a game's full record; answered with `Event::QueryResult`.
    LoadGame {
        /// Game.
        id: GameId,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// A statistics query; answered with `Event::QueryResult`.
    QueryStats {
        /// Request.
        request: StatsRequest,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Delete all play history (STATISTICS §9).
    ClearStatistics,
    /// Delete one game's play history.
    ResetGameStatistics {
        /// The game.
        id: GameId,
    },
    /// Delete one ended session from history.
    RemoveSession {
        /// The session.
        id: SessionId,
    },
    /// Add a program to a game (`AddExecutable`).
    AddExecutable {
        /// The game.
        id: GameId,
        /// The file.
        path: PathBuf,
        /// Its role.
        role: crate::model::ExeRole,
    },
    /// Point a moved program at its new place (*Locate…*, LOGIC_SPECS §8). `exe` defaults
    /// to the primary.
    LocateExecutable {
        /// The game.
        id: GameId,
        /// The program.
        exe: Option<ExecutableId>,
        /// The new path.
        new_path: PathBuf,
    },
    /// List the programs in a folder (GAME_LIBRARY §11); answered with `QueryResult::Folder`.
    ScanFolder {
        /// The folder.
        folder: PathBuf,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Measure the private Windows storage; answered with `QueryResult::PrefixSize`.
    MeasurePrefix {
        /// The game.
        id: GameId,
        /// Reply ID.
        reply_to: QueryId,
    },
    // ---- artwork
    /// Import artwork for one slot. `ArtSource::SteamCdn(0)` means "find the Steam app ID"
    /// (the chain of ADR-0018; only with `artwork.online = "steam"`).
    ImportArtwork {
        /// The game.
        id: GameId,
        /// The slot.
        slot: ArtSlot,
        /// Where from.
        source: ArtSource,
    },
    /// Fetch every slot from the Steam CDN on request ("Find artwork online").
    FetchOnlineArtwork {
        /// The game.
        id: GameId,
    },
    // ---- launching
    /// Launch a game (LOGIC_SPECS §3).
    Launch {
        /// The game.
        id: GameId,
        /// Which program (default: primary).
        exe: Option<ExecutableId>,
        /// Where it was requested.
        origin: LaunchOrigin,
    },
    /// Cancel a launch before its game starts (LOGIC_SPECS §3 "Cancellation"); from
    /// `Starting` on it stops the session.
    CancelLaunch {
        /// The game.
        id: GameId,
    },
    /// Run a tool in the game's private storage (SVC-005, a `Tool` session).
    RunTool {
        /// The game.
        id: GameId,
        /// The tool.
        tool: crate::model::Tool,
    },
    /// Stop a session (always accepted, idempotent; LOGIC_SPECS §6).
    StopSession {
        /// The session.
        session: SessionId,
        /// Graceful or forced.
        mode: StopMode,
    },
    /// Answer a prompt raised by a parked flow.
    AnswerPrompt {
        /// The prompt.
        prompt: PromptId,
        /// The decision.
        answer: PromptAnswer,
    },
    // ---- prefix
    /// Reset the private Windows storage (LOGIC_SPECS §12).
    ResetPrefix {
        /// The game.
        id: GameId,
        /// Dialog options.
        options: ResetOptions,
    },
    // ---- runtimes
    /// Install a build or runtime (M7; answers `EP-INT-003` until then).
    ProvisionRuntime {
        /// What.
        target: RuntimeRef,
    },
    /// Copy a Steam Linux Runtime variant that umu installed for the user into the store
    /// (`runtime::adopt`). Never writes outside the store.
    AdoptRuntime {
        /// The variant.
        variant: SlrVariant,
    },
    /// Remove a store build (M7).
    RemoveRuntime {
        /// The build.
        name: RuntimeName,
    },
    /// Rescan compatibility layers.
    RefreshRuntimes,
    /// Verify the runtimes (M7).
    VerifyRuntime,
    // ---- preferences and host
    /// Change preferences (LOGIC_SPECS §10); invalid patches change nothing.
    SetPreference(PreferencePatch),
    /// Probe the host again.
    RefreshHostFacts,
    // ---- desktop integration (DESKTOP_INTEGRATION §4, §6, §8)
    /// Become (or stop being) the `.exe` default handler; only on the user's request.
    SetDefaultHandler {
        /// `true`: take over (previous defaults recorded); `false`: restore them.
        enable: bool,
    },
    /// Add or remove a game's applications-menu shortcut.
    SetShortcut {
        /// The game.
        id: GameId,
        /// Create (`false`: remove).
        enable: bool,
    },
    /// Open a known folder in the file manager.
    OpenFolder(FolderRequest),
    /// Open a session log (the game's latest when `session` is `None`).
    ViewLog {
        /// The session.
        session: Option<SessionId>,
        /// The game.
        game: Option<GameId>,
    },
    // ---- lifecycle
    /// Stop the service: parked flows are cancelled, running games are left alone.
    Shutdown,
}

impl Command {
    /// A short name for logs.
    pub fn name(&self) -> &'static str {
        match self {
            Command::OpenPaths { .. } => "open_paths",
            Command::CreateGame { .. } => "create_game",
            Command::DiscardDraft => "discard_draft",
            Command::RemoveGame { .. } => "remove_game",
            Command::SetFavorite { .. } => "set_favorite",
            Command::SetTags { .. } => "set_tags",
            Command::SetHidden { .. } => "set_hidden",
            Command::UpdateGame { .. } => "update_game",
            Command::LoadGame { .. } => "load_game",
            Command::QueryStats { .. } => "query_stats",
            Command::ClearStatistics => "clear_statistics",
            Command::ResetGameStatistics { .. } => "reset_game_statistics",
            Command::RemoveSession { .. } => "remove_session",
            Command::AddExecutable { .. } => "add_executable",
            Command::LocateExecutable { .. } => "locate_executable",
            Command::ScanFolder { .. } => "scan_folder",
            Command::MeasurePrefix { .. } => "measure_prefix",
            Command::CancelLaunch { .. } => "cancel_launch",
            Command::RunTool { .. } => "run_tool",
            Command::ImportArtwork { .. } => "import_artwork",
            Command::FetchOnlineArtwork { .. } => "fetch_online_artwork",
            Command::Launch { .. } => "launch",
            Command::StopSession { .. } => "stop_session",
            Command::AnswerPrompt { .. } => "answer_prompt",
            Command::ResetPrefix { .. } => "reset_prefix",
            Command::ProvisionRuntime { .. } => "provision_runtime",
            Command::AdoptRuntime { .. } => "adopt_runtime",
            Command::RemoveRuntime { .. } => "remove_runtime",
            Command::RefreshRuntimes => "refresh_runtimes",
            Command::VerifyRuntime => "verify_runtime",
            Command::SetPreference(_) => "set_preference",
            Command::RefreshHostFacts => "refresh_host_facts",
            Command::SetDefaultHandler { .. } => "set_default_handler",
            Command::SetShortcut { .. } => "set_shortcut",
            Command::OpenFolder(_) => "open_folder",
            Command::ViewLog { .. } => "view_log",
            Command::Shutdown => "shutdown",
        }
    }
}
