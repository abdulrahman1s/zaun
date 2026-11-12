//! `UiBridge`: the UI's only connection to the application service. `send(UiCommand)`,
//! snapshot accessors and two pure helpers; the UI never blocks on it (`try_event` only).
//! See MODULES §8 and ARCHITECTURE §5.1, §6.
//!
//! # The seam (contract for the AppService)
//!
//! The seam is the [`Backend`] trait. Its vocabulary mirrors ARCHITECTURE §6 one-to-one so the
//! service adapter is a thin mapping; where a core type exists it is used as is.
//!
//! **Snapshots** (cheap `Arc` copies; refreshed when the matching event arrives):
//!
//! | Accessor | Type | Announced by |
//! |---|---|---|
//! | [`Backend::library`] | `Arc<`[`LibrarySnapshot`]`>` (core) | [`UiEvent::LibraryChanged`] |
//! | [`Backend::preferences`] | `Arc<`[`Preferences`]`>` (core) | [`UiEvent::PreferencesChanged`] |
//! | [`Backend::host`] | `Arc<`[`HostView`]`>` | [`UiEvent::HostChanged`] |
//! | [`Backend::runtimes`] | `Arc<`[`RuntimesView`]`>` | [`UiEvent::RuntimesChanged`] |
//! | [`Backend::diagnostics`] | `Arc<`[`Diagnostics`]`>` (core) | [`UiEvent::DiagnosticsChanged`] |
//!
//! **Pure helpers** (synchronous, sub-millisecond, no blocking I/O: they run on the UI thread
//! for every edit in the setup sheet and the Protection tab, SCREEN_SPECS §6):
//!
//! - [`Backend::assess`]: `sandbox::explain` statements and `sandbox::validate` issues for a
//!   policy as it would run for a game ([`PolicyAssessment`]); the service backend calls
//!   `sandbox::explain::explain_policy` (the real planner with a synthetic runtime and
//!   session; `stat` calls of the policy's paths only). The breadth scan is not part of it
//!   (a broad grant raises `PromptKind::ConfirmBroadGrant` when saved).
//! - [`Backend::preset_policy`]: `sandbox::apply_preset` with the current host facts.
//!
//! **Commands** ([`UiCommand`]) are queued and never answered synchronously. Their results
//! arrive as snapshots or as events:
//!
//! | Command | Service mapping | Result |
//! |---|---|---|
//! | `OpenPaths` | `Command::OpenPaths` | `SetupRequested`, `Prompt`, `Failure`, or a launch |
//! | `ScanFolder` | folder scan of GAME_LIBRARY §11 | `QueryResult(Folder)` |
//! | `CreateGame` | `Command::CreateGame` | `LibraryChanged` (+ `LaunchProgress` when `then = Launch`) |
//! | `DiscardDraft` | `Command::DiscardDraft` | the next queued `SetupRequested` |
//! | `UpdateGame` | `Command::UpdateGame` with a [`GameEdit`] list | `LibraryChanged`, `QueryResult(Game)` refresh |
//! | `RemoveGame`, `SetFavorite`, `SetHidden`, `SetTags`, `CreateTag` | library commands | `LibraryChanged` |
//! | `ApplyPreset` | bulk preset change (GAME_LIBRARY §9) keeping folders and `game_dir` | `LibraryChanged`, `Toast` summary |
//! | `AddExecutable`, `LocateExecutable`, `RescanMissing` | library commands | `LibraryChanged` |
//! | `ImportArtwork`, `ClearArtwork`, `FetchSteamArtwork` | artwork commands (ADR-0018) | `LibraryChanged` or `ArtworkFailed` |
//! | `Launch`, `CancelLaunch`, `Stop`, `RunTool` | launching | `LaunchProgress` |
//! | `AnswerPrompt` | `Command::AnswerPrompt` | flow continues |
//! | `ResetPrefix` | `Command::ResetPrefix` | `Toast` |
//! | `ProvisionRuntime`, `RemoveRuntime`, `RefreshRuntimes`, `VerifyRuntime` | runtimes | `RuntimesChanged` |
//! | `SetPreference` | `Command::SetPreference(patch)`; `preview` is the UI's optimistic result | `PreferencesChanged` |
//! | `RefreshHostFacts`, `SetDefaultHandler`, `CompleteOnboarding` | host and integration | `HostChanged`, `PreferencesChanged` |
//! | `QueryStats` | `Command::QueryStats` | `QueryResult(Stats)` |
//! | `LoadGame` | the full `Game` row plus storage facts | `QueryResult(Game)` |
//! | `MeasurePrefix`, `SearchFixes` | storage size (job pool), umu-database search (not yet) | `QueryResult(PrefixSize / Fixes)` |
//! | `ClearStatistics`, `ResetGameStatistics`, `RemoveSession` | statistics edits | `LibraryChanged`, `Toast` |
//! | `OpenFolder`, `ViewLog`, `CreateShortcut` | desktop actions done by the service | none / `Failure` |
//! | `Inspect`, `ListSessions`, `ReadLog`, `ListProcesses` | Developer Mode queries (DEVELOPER_MODE §3) | `QueryResult(Inspector / Sessions / LogChunk / Processes)` |
//! | `RunDoctor`, `BuildReport`, `SaveReport`, `DismissCrash`, `ResolveDatabase` | diagnostics (ERROR_HANDLING §5–7) | `DiagnosticsChanged`, `QueryResult(Report)`, `Toast` |
//! | `Quit` | `Command::Shutdown` | window closes |
//!
//! **Events** ([`UiEvent`]): `LibraryChanged`, `PreferencesChanged`, `HostChanged`,
//! `RuntimesChanged` (snapshots), `LaunchProgress { game, state }` (core `LaunchState`),
//! `SetupRequested { draft, context }` (core `GameDraft`), `Prompt` (core `PromptRequest`),
//! `Toast`, `Failure` (core `UserError`), `QueryResult { id, payload }`, `ArtworkFailed`,
//! `ShowGame`, `QuitRequested`.
//!
//! Three implementations ship with the crate: [`crate::service_backend::ServiceBackend`]
//! (the real `AppService`), [`crate::demo::DemoBackend`] (sample library, simulated
//! launches) and [`FakeBackend`] (records commands; tests publish snapshots and events).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use easyplay_core::clock::Timestamp;
use easyplay_core::config::{PreferencePatch, Preferences};
use easyplay_core::error::UserError;
use easyplay_core::ids::{
    ExecutableId, GameId, PromptId, QueryId, RuntimeName, SessionId, TagName,
};
use easyplay_core::launch::LaunchState;
use easyplay_core::library::{LibrarySnapshot, RemoveOptions, ResetOptions};
use easyplay_core::model::{
    ArtSlot, BasePreset, EntryKind, ExeRole, Game, GameSummary, LaunchConfig, PromptAnswer,
    PromptRequest, RuntimeBuild, RuntimeRef, SandboxPolicy, TagColor, Tool,
};
use easyplay_core::sandbox::{AccessStatement, PolicyIssue};
use easyplay_core::stats::{StatsRequest, StatsResult};

pub use easyplay_core::diagnostics::report::ReportFile;
pub use easyplay_core::launch::InspectorData;
pub use easyplay_core::library::TagInfo;
pub use easyplay_core::model::Session;
pub use easyplay_core::service::{
    DatabaseChoice, Diagnostics, InspectTarget, LogChunk, LogFile, RecoveryReason, RecoveryState,
};
pub use easyplay_core::session::ProcessInfo;

/// The library snapshot the UI reads (GAME_LIBRARY §1.1): the core type itself.
pub type LibraryView = LibrarySnapshot;

/// Build a snapshot from summaries (demo data, tests): sorted, indexed and counted exactly as
/// the service would.
pub fn library_from(
    games: Vec<GameSummary>,
    tags: Vec<(easyplay_core::ids::TagId, &str, TagColor)>,
    revision: u64,
    now: Timestamp,
) -> LibrarySnapshot {
    let mut s = LibrarySnapshot::empty(now);
    for g in games {
        s = s.with_game(g, now);
    }
    let tags = tags
        .into_iter()
        .map(|(id, name, color)| TagInfo {
            id,
            name: Arc::from(name),
            color,
            count: 0,
        })
        .collect();
    s = s.with_tags(tags, now);
    s.revision = revision;
    s
}

/// The host-status row (SCREEN_SPECS §0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HostStatus {
    /// Before the first host facts arrive.
    #[default]
    Unknown,
    /// All checks pass.
    Ready,
    /// A check needs the user (opens Doctor).
    NeedsAttention,
    /// Background jobs running for more than 3 s.
    Busy,
}

/// Outcome of one system check (onboarding, Doctor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckState {
    /// Still running.
    Pending,
    /// Fine.
    Pass,
    /// Works with a caveat.
    Warn,
    /// Blocks launching.
    Fail,
}

/// One row of the system check (SCREEN_SPECS §1). Mapped by the service from `host::doctor`
/// checks; texts are already the user's words for this distribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCheck {
    /// Stable ID (`D-04`).
    pub id: String,
    /// Status.
    pub state: CheckState,
    /// What is checked ("Sandbox tool").
    pub name: String,
    /// The result ("bubblewrap 0.12", "Turned off on this system").
    pub result: String,
    /// Explanation shown under a failure.
    pub detail: Option<String>,
    /// A command that fixes it (copyable, `mono`).
    pub fix_command: Option<String>,
}

/// Whether EasyPlay opens Windows programs (USER_FLOWS UF-17).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HandlerState {
    /// Not determined yet.
    #[default]
    Unknown,
    /// EasyPlay is the default.
    EasyPlay,
    /// Another application is (its name).
    Other(String),
    /// Nothing handles `.exe`.
    None,
}

/// A desktop-menu shortcut EasyPlay created (Settings → Integration).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutInfo {
    /// The game.
    pub game: GameId,
    /// Shown name.
    pub name: String,
}

/// Host facts the screens show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostView {
    /// The status row.
    pub status: HostStatus,
    /// System checks in display order.
    pub checks: Vec<HostCheck>,
    /// `.exe` handler.
    pub handler: HandlerState,
    /// Distribution name for fix texts ("Fedora 41").
    pub distro: String,
    /// Background jobs running (activity popover).
    pub jobs: Vec<String>,
    /// Menu shortcuts created by EasyPlay.
    pub shortcuts: Vec<ShortcutInfo>,
    /// The disk supports reflinks (explains the isolation mode).
    pub reflinks: bool,
    /// The user can read keyboards directly (disables "All input devices").
    pub input_readable: bool,
    /// MangoHud is installed.
    pub mangohud: bool,
    /// The desktop uses X11.
    pub x11: bool,
    /// EasyPlay version.
    pub version: String,
    /// The home folder (locations are shown relative to it, "in Home").
    pub home: PathBuf,
    /// The well-known folders' real paths (XDG user directories).
    pub user_dirs: Vec<(easyplay_core::model::UserDir, PathBuf)>,
}

impl HostView {
    /// Whether a blocking check failed.
    pub fn has_blocking_failure(&self) -> bool {
        self.checks.iter().any(|c| c.state == CheckState::Fail)
    }
}

/// A compatibility-layer build with the facts Settings shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerInfo {
    /// The build (core).
    pub build: RuntimeBuild,
    /// Size on disk.
    pub size: Option<u64>,
    /// Games using it.
    pub used_by: u32,
}

/// A download in progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadInfo {
    /// What ("GE-Proton 10-25").
    pub label: String,
    /// Bytes so far.
    pub bytes: u64,
}

/// The runtime catalogue as the UI shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimesView {
    /// Installed builds.
    pub layers: Vec<LayerInfo>,
    /// The label of what "Default" resolves to now ("UMU-Proton 10.0-4"), `None` when nothing
    /// is installed yet.
    pub default_label: Option<String>,
    /// Steam Linux Runtime variants: (name, version, last verified).
    pub slr: Vec<(String, String, Option<Timestamp>)>,
    /// Last update check.
    pub last_checked: Option<Timestamp>,
    /// Downloads in progress.
    pub downloads: Vec<DownloadInfo>,
}

/// A policy as it would run, explained (SCREEN_SPECS §6, SCREEN_SPECS_2 §8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PolicyAssessment {
    /// `sandbox::explain` statements.
    pub statements: Vec<AccessStatement>,
    /// `sandbox::validate` issues.
    pub issues: Vec<PolicyIssue>,
}

/// Where the paths of `OpenPaths` came from (LOGIC_SPECS §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpenOrigin {
    /// The *Add game* picker or the add-game sheet.
    Picker,
    /// Dropped onto the window.
    DragDrop,
    /// The file manager (quick launch).
    FileManager,
}

/// Where setup was started (the primary button differs, SCREEN_SPECS §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetupOrigin {
    /// From the file manager: primary is **Play**.
    FileManager,
    /// From inside EasyPlay: primary is **Add to library**.
    InsideApp,
}

/// Context of a setup request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SetupContext {
    /// Origin.
    pub origin: SetupOrigin,
    /// Position in a batch (1-based) and the batch size.
    pub queue: (u32, u32),
}

/// What happens after `CreateGame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AfterCreate {
    /// Only save.
    Nothing,
    /// Save and start.
    Launch,
    /// Save, and use the same settings for the rest of the batch.
    ApplyToRest,
}

/// One change to a game (`Command::UpdateGame { patch }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameEdit {
    /// Rename (1–80 characters).
    Name(String),
    /// Back to the detected name.
    ResetName,
    /// Notes text.
    Notes(String),
    /// The whole sandbox policy.
    Policy(Box<SandboxPolicy>),
    /// The whole launch configuration.
    Launch(Box<LaunchConfig>),
    /// Compatibility layer.
    Runtime {
        /// Reference.
        reference: RuntimeRef,
        /// Follow updates of a channel.
        follow: bool,
    },
    /// Game fix entry (`None` = no fix).
    UmuId(Option<String>),
    /// Type.
    Kind(EntryKind),
    /// Steam app ID for artwork (user-set).
    SteamAppId(Option<u32>),
    /// Program role.
    ExeRole(ExecutableId, ExeRole),
    /// Program label.
    ExeLabel(ExecutableId, Option<String>),
    /// A program's own arguments (`None` = the game's arguments).
    ExeArgs(ExecutableId, Option<Vec<String>>),
    /// Remove a program from the game.
    RemoveExe(ExecutableId),
}

/// Where an artwork image comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtworkSource {
    /// An image file the user chose or dropped.
    File(PathBuf),
    /// The primary program's icon.
    ProgramIcon,
}

/// Folders the service opens in the file manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderTarget {
    /// The game folder.
    GameFolder(GameId),
    /// The save data.
    SaveData(GameId),
    /// The private Windows storage.
    Prefix(GameId),
    /// A session's folder.
    Session(SessionId),
    /// EasyPlay's data folder.
    DataFolder,
    /// The library backups folder.
    Backups,
    /// Any path the user can already see (a granted folder).
    Path(PathBuf),
}

/// What the add-game sheet lists (SCREEN_SPECS §5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramFound {
    /// Path.
    pub path: PathBuf,
    /// Role guess.
    pub role: ExeRole,
    /// Looks like an uninstaller, redistributable or crash handler (shown under a disclosure).
    pub filtered: bool,
    /// Size in bytes.
    pub size: u64,
}

/// Result of scanning a folder for programs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderScan {
    /// The folder.
    pub folder: PathBuf,
    /// Candidates, best first.
    pub programs: Vec<ProgramFound>,
    /// The scan stopped at the entry limit.
    pub truncated: bool,
    /// Still scanning (more results follow under the same query ID).
    pub scanning: bool,
    /// The folder could not be read (the reason).
    pub error: Option<String>,
}

/// A game-fix database entry (Compatibility → Game fixes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixEntry {
    /// `umu-…` ID.
    pub umu_id: String,
    /// Title.
    pub title: String,
    /// Store name.
    pub store: String,
}

/// Everything about one game beyond its summary (game page tabs).
#[derive(Clone, Debug, PartialEq)]
pub struct GameDetail {
    /// The full record (core).
    pub game: Game,
    /// Where the save data lives, as the user would describe it.
    pub save_location: Option<PathBuf>,
    /// When the private storage was created.
    pub prefix_created_at: Option<Timestamp>,
    /// The build "Default" or the channel resolves to now.
    pub runtime_resolved: Option<String>,
    /// The selected layer is no longer installed.
    pub runtime_missing: bool,
    /// The selected layer is older than the one the storage was prepared with.
    pub runtime_older: bool,
    /// The layer has no game fixes (Valve Proton).
    pub runtime_without_fixes: bool,
    /// The fix entry's title.
    pub fix_title: Option<String>,
    /// The game has a menu shortcut.
    pub has_shortcut: bool,
}

/// What a query returns.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryPayload {
    /// Statistics.
    Stats(StatsResult),
    /// A game's full record.
    Game(Box<GameDetail>),
    /// Programs in a folder.
    Folder(FolderScan),
    /// Private-storage size in bytes.
    PrefixSize(u64),
    /// Game-fix search results.
    Fixes(Vec<FixEntry>),
    /// The launch inspector's data.
    Inspector(Box<InspectorData>),
    /// A game's recent sessions, newest first.
    Sessions(Vec<Session>),
    /// A chunk of a session log.
    LogChunk(LogChunk),
    /// A session's processes now.
    Processes(Vec<ProcessInfo>),
    /// The diagnostics report's files, for the preview.
    Report(Vec<ReportFile>),
    /// The query failed.
    Failed(UserError),
}

/// Toast emphasis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    /// Neutral.
    Info,
    /// A completed action.
    Success,
    /// Something did not work, nothing to decide.
    Warning,
}

/// A toast from the service.
#[derive(Clone, Debug, PartialEq)]
pub struct ToastMsg {
    /// One line.
    pub text: String,
    /// Emphasis.
    pub level: ToastLevel,
    /// Undo: the command that reverses the action.
    pub undo: Option<Box<UiCommand>>,
}

/// Commands the UI sends (ARCHITECTURE §6 `Command`, see the module docs for the mapping).
#[derive(Clone, Debug, PartialEq)]
pub enum UiCommand {
    /// Open programs: setup, prompts or a launch follow.
    OpenPaths {
        /// Files or folders.
        paths: Vec<PathBuf>,
        /// Where they came from.
        origin: OpenOrigin,
    },
    /// List programs in a folder (add-game sheet).
    ScanFolder {
        /// Folder.
        folder: PathBuf,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Save a draft from the setup sheet.
    CreateGame {
        /// The edited draft.
        draft: Box<easyplay_core::library::GameDraft>,
        /// Then.
        then: AfterCreate,
    },
    /// The setup sheet closed without saving (`Command::DiscardDraft`; the next queued
    /// draft follows).
    DiscardDraft,
    /// Change a game.
    UpdateGame {
        /// Game.
        id: GameId,
        /// Changes, applied together.
        edits: Vec<GameEdit>,
    },
    /// Remove a game.
    RemoveGame {
        /// Game.
        id: GameId,
        /// What to delete.
        options: RemoveOptions,
    },
    /// Favourite.
    SetFavorite {
        /// Game.
        id: GameId,
        /// On.
        favorite: bool,
    },
    /// Hide.
    SetHidden {
        /// Game.
        id: GameId,
        /// Hidden.
        hidden: bool,
    },
    /// Replace a game's tags.
    SetTags {
        /// Game.
        id: GameId,
        /// Tags by name (new names create tags).
        tags: Vec<TagName>,
    },
    /// Switch games to a preset, keeping folders and game-folder access; running games are
    /// skipped (GAME_LIBRARY §9).
    ApplyPreset {
        /// Games.
        ids: Vec<GameId>,
        /// Preset.
        preset: BasePreset,
    },
    /// Add a program to a game.
    AddExecutable {
        /// Game.
        id: GameId,
        /// File.
        path: PathBuf,
        /// Role.
        role: ExeRole,
    },
    /// Point a moved program at its new place (*Locate…*).
    LocateExecutable {
        /// Game.
        id: GameId,
        /// The program (`None` = the primary).
        exe: Option<ExecutableId>,
        /// New path.
        new_path: PathBuf,
    },
    /// Re-check missing games (`F5`).
    RescanMissing,
    /// Set artwork.
    ImportArtwork {
        /// Game.
        id: GameId,
        /// Slot.
        slot: ArtSlot,
        /// Source.
        source: ArtworkSource,
    },
    /// Remove artwork from a slot.
    ClearArtwork {
        /// Game.
        id: GameId,
        /// Slot.
        slot: ArtSlot,
    },
    /// Fetch official artwork from the Steam CDN (ADR-0018); `app_id` set by the user wins.
    FetchSteamArtwork {
        /// Game.
        id: GameId,
        /// User-entered app ID.
        app_id: Option<u32>,
    },
    /// Start a game.
    Launch {
        /// Game.
        id: GameId,
        /// A specific program (Launch options → run).
        exe: Option<ExecutableId>,
    },
    /// Cancel a launch before the game starts.
    CancelLaunch {
        /// Game.
        id: GameId,
    },
    /// Stop a running game.
    Stop {
        /// Game.
        id: GameId,
        /// Force (after the grace period).
        force: bool,
    },
    /// Run a tool in the game's storage.
    RunTool {
        /// Game.
        id: GameId,
        /// Tool.
        tool: Tool,
    },
    /// Answer a prompt.
    AnswerPrompt {
        /// Prompt.
        prompt: PromptId,
        /// Answer.
        answer: PromptAnswer,
    },
    /// Reset the private Windows storage.
    ResetPrefix {
        /// Game.
        id: GameId,
        /// Options.
        options: ResetOptions,
    },
    /// Download a compatibility layer.
    ProvisionRuntime {
        /// What.
        target: RuntimeRef,
    },
    /// Delete an installed layer.
    RemoveRuntime {
        /// Build.
        name: RuntimeName,
    },
    /// Check for layer updates.
    RefreshRuntimes,
    /// Verify the Steam Linux Runtime.
    VerifyRuntime,
    /// Change preferences.
    SetPreference {
        /// The change (authoritative).
        patch: PreferencePatch,
        /// The UI's optimistic result of applying it.
        preview: Box<Preferences>,
    },
    /// Re-run the system checks.
    RefreshHostFacts,
    /// Become (or stop being) the `.exe` handler.
    SetDefaultHandler {
        /// Enable.
        enable: bool,
    },
    /// Onboarding finished or skipped.
    CompleteOnboarding {
        /// Skipped with a failing check (leaves a banner).
        skipped: bool,
    },
    /// A statistics query.
    QueryStats {
        /// Request.
        request: StatsRequest,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Load a game's full record.
    LoadGame {
        /// Game.
        id: GameId,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Measure the private storage.
    MeasurePrefix {
        /// Game.
        id: GameId,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Search the game-fix database.
    SearchFixes {
        /// Text.
        text: String,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Delete all play history.
    ClearStatistics,
    /// Delete one game's play history.
    ResetGameStatistics {
        /// Game.
        id: GameId,
    },
    /// Delete one session from history.
    RemoveSession {
        /// Session.
        id: SessionId,
    },
    /// Open a folder in the file manager.
    OpenFolder(FolderTarget),
    /// Open a session log.
    ViewLog {
        /// Session (`None`: the latest of the game).
        session: Option<SessionId>,
        /// Game.
        game: Option<GameId>,
    },
    /// Add or remove the applications-menu shortcut.
    SetShortcut {
        /// Game.
        id: GameId,
        /// Create.
        enable: bool,
    },
    /// Launch inspector data (dry run or session).
    Inspect {
        /// What.
        target: InspectTarget,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// A game's recent sessions.
    ListSessions {
        /// Game.
        game: GameId,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Read a chunk of a session log file.
    ReadLog {
        /// Session.
        session: SessionId,
        /// File.
        file: LogFile,
        /// Offset.
        offset: u64,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// A session's processes (the process tree, refreshed while visible).
    ListProcesses {
        /// Session.
        session: SessionId,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Run every Doctor check.
    RunDoctor,
    /// Collect the diagnostics report for the preview.
    BuildReport {
        /// The game to include.
        game: Option<GameId>,
        /// The session to include (default: the game's latest).
        session: Option<SessionId>,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Save the previewed report.
    SaveReport {
        /// The files.
        files: Vec<ReportFile>,
        /// Where.
        dest: PathBuf,
    },
    /// Dismiss the crash banner.
    DismissCrash,
    /// Answer the damaged-library choice.
    ResolveDatabase(DatabaseChoice),
    /// Quit EasyPlay (games keep running).
    Quit,
}

/// Events the UI drains (ARCHITECTURE §6 `Event`).
#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    /// A new library snapshot is available.
    LibraryChanged,
    /// New preferences.
    PreferencesChanged,
    /// Host facts changed.
    HostChanged,
    /// The runtime catalogue changed.
    RuntimesChanged,
    /// The diagnostics snapshot changed (Doctor, crash banner, recovery mode).
    DiagnosticsChanged,
    /// A launch moved to a new state.
    LaunchProgress {
        /// Game.
        game: GameId,
        /// State (core).
        state: LaunchState,
    },
    /// Show the setup sheet for an unknown program.
    SetupRequested {
        /// Draft (core).
        draft: Box<easyplay_core::library::GameDraft>,
        /// Context.
        context: SetupContext,
    },
    /// A decision is needed.
    Prompt(PromptRequest),
    /// Show a toast.
    Toast(ToastMsg),
    /// A failure not attached to a launch.
    Failure(UserError),
    /// A query answered.
    QueryResult {
        /// The request's ID.
        id: QueryId,
        /// Payload.
        payload: QueryPayload,
    },
    /// An artwork import failed (shown inline on the slot).
    ArtworkFailed {
        /// Game.
        game: GameId,
        /// Slot.
        slot: ArtSlot,
        /// Why.
        error: UserError,
    },
    /// Show a game's page (a known program was opened or dropped).
    ShowGame(GameId),
    /// The service asks the window to close.
    QuitRequested,
}

/// What the UI needs from the application service. Implementations must be cheap and
/// non-blocking: `send` queues, `try_event` never waits, snapshot accessors copy an `Arc`,
/// `assess` and `preset_policy` are pure.
pub trait Backend: Send {
    /// Queue a command.
    fn send(&self, cmd: UiCommand);
    /// The next pending event, if any.
    fn try_event(&self) -> Option<UiEvent>;
    /// The current library snapshot.
    fn library(&self) -> Arc<LibrarySnapshot>;
    /// The current preferences.
    fn preferences(&self) -> Arc<Preferences>;
    /// Host facts.
    fn host(&self) -> Arc<HostView>;
    /// The runtime catalogue.
    fn runtimes(&self) -> Arc<RuntimesView>;
    /// Explain and validate `policy` as it would run for `game` (or a draft at `root`).
    fn assess(
        &self,
        game: Option<GameId>,
        name: &str,
        root: Option<&std::path::Path>,
        policy: &SandboxPolicy,
    ) -> PolicyAssessment;
    /// `policy` switched to `preset` on this host (keeps folders and game-folder access).
    fn preset_policy(&self, preset: BasePreset, policy: &SandboxPolicy) -> SandboxPolicy;
    /// Install the repaint waker (`egui::Context::request_repaint`) for other threads.
    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>);
    /// Whether this is sample data (shows the preview banner).
    fn is_preview(&self) -> bool {
        false
    }
    /// Doctor report, crash record, recovery mode.
    fn diagnostics(&self) -> Arc<Diagnostics> {
        Arc::new(Diagnostics::default())
    }
}

/// Maximum events drained per frame (ARCHITECTURE §5.1).
pub const MAX_EVENTS_PER_FRAME: usize = 256;

/// The UI side of the seam: caches the current snapshots for the frame.
pub struct UiBridge {
    backend: Box<dyn Backend>,
    library: Arc<LibrarySnapshot>,
    prefs: Arc<Preferences>,
    host: Arc<HostView>,
    runtimes: Arc<RuntimesView>,
    diagnostics: Arc<Diagnostics>,
    next_query: std::cell::Cell<u128>,
}

impl UiBridge {
    /// Wrap a backend and take the first snapshots.
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            library: backend.library(),
            prefs: backend.preferences(),
            host: backend.host(),
            runtimes: backend.runtimes(),
            diagnostics: backend.diagnostics(),
            backend,
            next_query: std::cell::Cell::new(1),
        }
    }

    /// Send a command (never blocks).
    pub fn send(&self, cmd: UiCommand) {
        self.backend.send(cmd);
    }

    /// A fresh query ID.
    pub fn query_id(&self) -> QueryId {
        let n = self.next_query.get();
        self.next_query.set(n + 1);
        QueryId::from_uuid(uuid::Uuid::from_u128(
            0xE0E0_0000_0000_0000_0000_0000_0000_0000 | n,
        ))
    }

    /// Drain up to [`MAX_EVENTS_PER_FRAME`] events, refreshing snapshots as announced.
    /// Returns every event (snapshot events too, so view-models can react).
    pub fn drain(&mut self) -> Vec<UiEvent> {
        let mut out = Vec::new();
        for _ in 0..MAX_EVENTS_PER_FRAME {
            let Some(ev) = self.backend.try_event() else {
                break;
            };
            match &ev {
                UiEvent::LibraryChanged => self.library = self.backend.library(),
                UiEvent::HostChanged => self.host = self.backend.host(),
                UiEvent::PreferencesChanged => self.prefs = self.backend.preferences(),
                UiEvent::RuntimesChanged => self.runtimes = self.backend.runtimes(),
                UiEvent::DiagnosticsChanged => self.diagnostics = self.backend.diagnostics(),
                _ => {}
            }
            out.push(ev);
        }
        out
    }

    /// The library snapshot of this frame.
    pub fn library(&self) -> &Arc<LibrarySnapshot> {
        &self.library
    }

    /// Preferences of this frame.
    pub fn prefs(&self) -> &Arc<Preferences> {
        &self.prefs
    }

    /// Replace the preferences optimistically (Appearance changes apply at once).
    pub fn set_prefs_preview(&mut self, prefs: Preferences) {
        self.prefs = Arc::new(prefs);
    }

    /// Host facts of this frame.
    pub fn host(&self) -> &Arc<HostView> {
        &self.host
    }

    /// Host status of this frame.
    pub fn host_status(&self) -> HostStatus {
        self.host.status
    }

    /// Runtime catalogue of this frame.
    pub fn runtimes(&self) -> &Arc<RuntimesView> {
        &self.runtimes
    }

    /// Diagnostics of this frame.
    pub fn diagnostics(&self) -> &Arc<Diagnostics> {
        &self.diagnostics
    }

    /// See [`Backend::assess`].
    pub fn assess(
        &self,
        game: Option<GameId>,
        name: &str,
        root: Option<&std::path::Path>,
        policy: &SandboxPolicy,
    ) -> PolicyAssessment {
        self.backend.assess(game, name, root, policy)
    }

    /// See [`Backend::preset_policy`].
    pub fn preset_policy(&self, preset: BasePreset, policy: &SandboxPolicy) -> SandboxPolicy {
        self.backend.preset_policy(preset, policy)
    }

    /// Whether the backend serves sample data.
    pub fn is_preview(&self) -> bool {
        self.backend.is_preview()
    }

    /// Pass the repaint waker to the backend.
    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.backend.set_waker(waker);
    }
}

struct FakeState {
    sent: Vec<UiCommand>,
    events: VecDeque<UiEvent>,
    library: Arc<LibrarySnapshot>,
    prefs: Arc<Preferences>,
    host: Arc<HostView>,
    runtimes: Arc<RuntimesView>,
    diagnostics: Arc<Diagnostics>,
    assess: Option<PolicyAssessment>,
}

/// A test backend: records commands; tests publish snapshots and events. `assess` uses the
/// demo explainer unless a fixed assessment is set.
#[derive(Clone)]
pub struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self::new(LibrarySnapshot::empty(crate::demo::baseline_now()))
    }
}

impl FakeBackend {
    /// A backend serving `library`, default preferences and a ready host.
    pub fn new(library: LibrarySnapshot) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                sent: Vec::new(),
                events: VecDeque::new(),
                library: Arc::new(library),
                prefs: Arc::new(crate::demo::preferences()),
                host: Arc::new(crate::demo::host_view()),
                runtimes: Arc::new(crate::demo::runtimes_view()),
                diagnostics: Arc::new(Diagnostics::default()),
                assess: None,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Commands sent so far.
    pub fn sent(&self) -> Vec<UiCommand> {
        self.lock().sent.clone()
    }

    /// Record a command as if the UI had sent it (test helpers that filter commands).
    pub fn record(&self, cmd: UiCommand) {
        self.lock().sent.push(cmd);
    }

    /// Forget recorded commands.
    pub fn clear_sent(&self) {
        self.lock().sent.clear();
    }

    /// Publish a new library and announce it.
    pub fn publish_library(&self, mut library: LibrarySnapshot) {
        let mut s = self.lock();
        library.revision = s.library.revision + 1;
        s.library = Arc::new(library);
        s.events.push_back(UiEvent::LibraryChanged);
    }

    /// Publish preferences and announce them.
    pub fn publish_prefs(&self, prefs: Preferences) {
        let mut s = self.lock();
        s.prefs = Arc::new(prefs);
        s.events.push_back(UiEvent::PreferencesChanged);
    }

    /// Publish host facts and announce them.
    pub fn publish_host(&self, host: HostView) {
        let mut s = self.lock();
        s.host = Arc::new(host);
        s.events.push_back(UiEvent::HostChanged);
    }

    /// Set the host status and announce it.
    pub fn set_host(&self, status: HostStatus) {
        let mut h = (*self.lock().host).clone();
        h.status = status;
        self.publish_host(h);
    }

    /// Publish diagnostics and announce them.
    pub fn publish_diagnostics(&self, d: Diagnostics) {
        let mut s = self.lock();
        s.diagnostics = Arc::new(d);
        s.events.push_back(UiEvent::DiagnosticsChanged);
    }

    /// Use a fixed assessment instead of the demo explainer.
    pub fn set_assessment(&self, a: PolicyAssessment) {
        self.lock().assess = Some(a);
    }

    /// Queue an event.
    pub fn push_event(&self, ev: UiEvent) {
        self.lock().events.push_back(ev);
    }
}

impl Backend for FakeBackend {
    fn send(&self, cmd: UiCommand) {
        self.lock().sent.push(cmd);
    }
    fn try_event(&self) -> Option<UiEvent> {
        self.lock().events.pop_front()
    }
    fn library(&self) -> Arc<LibrarySnapshot> {
        self.lock().library.clone()
    }
    fn preferences(&self) -> Arc<Preferences> {
        self.lock().prefs.clone()
    }
    fn host(&self) -> Arc<HostView> {
        self.lock().host.clone()
    }
    fn runtimes(&self) -> Arc<RuntimesView> {
        self.lock().runtimes.clone()
    }
    fn assess(
        &self,
        _game: Option<GameId>,
        name: &str,
        root: Option<&std::path::Path>,
        policy: &SandboxPolicy,
    ) -> PolicyAssessment {
        if let Some(a) = &self.lock().assess {
            return a.clone();
        }
        crate::demo::assess(name, root, policy)
    }
    fn preset_policy(&self, preset: BasePreset, policy: &SandboxPolicy) -> SandboxPolicy {
        crate::demo::preset_policy(preset, policy)
    }
    fn set_waker(&self, _waker: Arc<dyn Fn() + Send + Sync>) {}
    fn diagnostics(&self) -> Arc<Diagnostics> {
        self.lock().diagnostics.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_refreshes_snapshots_and_returns_events() {
        let fake = FakeBackend::default();
        let mut bridge = UiBridge::new(Box::new(fake.clone()));
        assert_eq!(bridge.library().revision, 0);
        fake.publish_library(LibrarySnapshot::empty(crate::demo::baseline_now()));
        fake.push_event(UiEvent::Toast(ToastMsg {
            text: "hi".into(),
            level: ToastLevel::Info,
            undo: None,
        }));
        fake.set_host(HostStatus::NeedsAttention);
        let evs = bridge.drain();
        assert_eq!(evs.len(), 3);
        assert_eq!(bridge.library().revision, 1);
        assert_eq!(bridge.host_status(), HostStatus::NeedsAttention);
        bridge.send(UiCommand::RescanMissing);
        assert_eq!(fake.sent(), vec![UiCommand::RescanMissing]);
        assert_ne!(bridge.query_id(), bridge.query_id());
    }
}
