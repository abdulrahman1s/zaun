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
    /// The library backups folder.
    Backups,
    /// A folder granted to some game (refused otherwise).
    Granted(PathBuf),
}

/// Which launch the inspector shows (DEVELOPER_MODE §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectTarget {
    /// A dry run of the game's next launch (`Command::DryRunPlan` of the spec).
    NextLaunch {
        /// The game.
        game: GameId,
        /// The program (default: primary).
        exe: Option<ExecutableId>,
    },
    /// An open or ended session (from its retained plan file).
    Session(SessionId),
}

/// A file of a session's log folder the viewer can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogFile {
    /// `game.log`.
    Game,
    /// `events.jsonl`.
    Events,
    /// `supervisor.log`.
    Supervisor,
    /// The Proton log (`proton/steam-*.log`), written when the game's *Proton log* option was
    /// on: Wine's debug output.
    Proton,
}

impl LogFile {
    /// The file name (for [`LogFile::Proton`], the folder Proton writes its log into).
    pub const fn file_name(self) -> &'static str {
        match self {
            LogFile::Game => "game.log",
            LogFile::Events => "events.jsonl",
            LogFile::Supervisor => "supervisor.log",
            LogFile::Proton => "proton",
        }
    }
}

/// Chat (ADR-0033). Answers arrive as `Event::ChatChanged` (the snapshot),
/// `Notice::ChatMessages` / `Notice::ChatMessagesUpdated`, and `QueryResult::Chat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatCommand {
    /// Turn Chat on (with the name friends see, for a first start) or off
    /// (`chat.enabled`, `chat.name`).
    SetEnabled {
        /// On or off.
        on: bool,
        /// The name for a first start (`None` = keep `chat.name`).
        name: Option<String>,
    },
    /// Change the name friends see.
    SetName(String),
    /// Make a one-time invitation link (`ChatSnapshot::invite`).
    CreateInvite,
    /// Use a friend's link.
    Connect(String),
    /// Accept a request made through the user's address.
    Accept(i64),
    /// Decline it.
    Reject(i64),
    /// Make the user's reusable address (`ChatSnapshot::address`).
    CreateAddress,
    /// Delete it.
    DeleteAddress,
    /// Send text.
    Send {
        /// Where.
        chat: crate::model::ChatRef,
        /// What.
        text: String,
        /// The message it answers.
        reply_to: Option<i64>,
        /// Members named with @ (name as written, member number).
        mentions: Vec<(String, i64)>,
    },
    /// Send a voice message recorded in the composer (the composer agent; the preview plays
    /// it, the real service ignores it until recording is carried).
    SendVoice {
        /// Where.
        chat: crate::model::ChatRef,
        /// Its length in seconds (1 to 300).
        secs: u32,
    },
    /// Send a picture or GIF file (core agent). The service copies it into the Chat folder
    /// (the only folder the engine's sandbox sees) and sends the copy; `.gif` goes as a GIF,
    /// anything else as a picture. Files over `chat::MAX_FILE` (1 MiB, what friends receive
    /// without asking) are refused. Gated by `ChatFeatures::files` (and `gifs` for GIFs).
    SendFile {
        /// Where.
        chat: crate::model::ChatRef,
        /// The file on this computer.
        path: PathBuf,
    },
    /// Change the text of one of the user's messages.
    Edit {
        /// Where.
        chat: crate::model::ChatRef,
        /// Which message.
        item: i64,
        /// The new text.
        text: String,
    },
    /// Delete one of the user's messages for everyone.
    Delete {
        /// Where.
        chat: crate::model::ChatRef,
        /// Which message.
        item: i64,
    },
    /// Add or take back a reaction.
    React {
        /// Where.
        chat: crate::model::ChatRef,
        /// Which message.
        item: i64,
        /// The emoji.
        emoji: String,
        /// Add (or remove).
        on: bool,
    },
    /// Mute or unmute a conversation's notifications.
    SetMuted {
        /// Which.
        chat: crate::model::ChatRef,
        /// Muted.
        on: bool,
    },
    /// Block or unblock a friend (they are not told).
    SetBlocked {
        /// The friend.
        friend: i64,
        /// Blocked.
        on: bool,
    },
    /// Block or unblock a group member (their messages collapse).
    SetMemberBlocked {
        /// The group.
        group: i64,
        /// The member.
        member: i64,
        /// Blocked.
        on: bool,
    },
    /// Fetch a friend's security code into `ChatSnapshot::code`.
    ShowCode(i64),
    /// The codes matched: mark the friend verified.
    Verify {
        /// The friend.
        friend: i64,
        /// The code that was compared.
        code: String,
    },
    /// The user's profile (name, about, avatar pair).
    SetProfile(crate::model::ChatProfile),
    /// The user's status (online, away, do not disturb, invisible).
    SetStatus(crate::model::MyStatus),
    /// Call a friend, or start or join a group's voice.
    Call(crate::model::ChatRef),
    /// Answer the ringing call.
    Answer,
    /// Decline the ringing call.
    Decline,
    /// Leave the call.
    EndCall,
    /// Turn the microphone off or on in the call.
    CallMute(bool),
    /// Stop or start hearing the call.
    CallDeafen(bool),
    /// Share the running game with a conversation.
    StartShare(crate::model::ChatRef),
    /// Stop sharing.
    StopShare,
    /// Watch the stream in a conversation.
    Watch(crate::model::ChatRef),
    /// Stop watching.
    StopWatch,
    /// Turn the watched stream's sound off or on.
    WatchMute(bool),
    /// Open a link from a message in the browser; with `chat.check_links` on, after the
    /// site is checked (a harmful one comes back as `Notice::ChatUnsafeLink` instead).
    OpenLink(String),
    /// Open a link without the check: the user chose "Open anyway" after
    /// `Notice::ChatUnsafeLink` (only `http` and `https`).
    OpenLinkAnyway(String),
    /// Search Giphy for GIFs (only the words are sent; `crate::gifs`), answered with
    /// `QueryResult::Gifs` to `reply_to`. Nothing is sent while `chat.gif_search` is off or
    /// no key is known (the answer says why).
    GifSearch {
        /// The words typed.
        query: String,
        /// Where the answer goes.
        reply_to: QueryId,
    },
    /// Giphy's trending GIFs, answered with `QueryResult::Gifs` (same rules as search).
    GifTrending {
        /// Where the answer goes.
        reply_to: QueryId,
    },
    /// A picker picture from Giphy's media hosts, answered with `QueryResult::GifPreview`.
    GifPreview {
        /// `crate::gifs::Gif::preview_url`.
        url: String,
        /// Where the answer goes.
        reply_to: QueryId,
    },
    /// Send a GIF: one from Giphy is downloaded first (at most `crate::gifs::MAX_SEND`) and
    /// sent as a file (`SendFile`), so friends never contact Giphy. A failure comes back as
    /// `Notice::ChatGifNotSent`.
    SendGif {
        /// Where.
        chat: crate::model::ChatRef,
        /// Which.
        gif: crate::gifs::GifPick,
    },
    /// Everything in the conversation was seen.
    MarkRead(crate::model::ChatRef),
    /// Messages of a conversation, answered with `QueryResult::Chat` to `reply_to`.
    LoadChat {
        /// Which.
        chat: crate::model::ChatRef,
        /// Only older than this message (`None` = the latest).
        before: Option<i64>,
        /// Where the answer goes.
        reply_to: QueryId,
    },
    /// Make a group.
    NewGroup(String),
    /// Invite a friend into a group.
    AddMember {
        /// The group.
        group: i64,
        /// The friend.
        friend: i64,
    },
    /// Join a group the user was invited to.
    JoinGroup(i64),
    /// Leave a group.
    LeaveGroup(i64),
    /// Remove a friend and the conversation.
    RemoveFriend(i64),
}

/// Steam games (ADR-0031). Favourite, hidden, tags, notes (`UpdateGame` with
/// `GameEdit::Notes`), Play (`Launch`) and folders (`OpenFolder`) use the general commands
/// with the game's [`GameId::steam`] ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SteamCommand {
    /// Turn "Show my Steam games" on or off (`steam.show_games`).
    SetEnabled(bool),
    /// Read Steam's files again now.
    Rescan,
    /// "Update in Steam": show Steam's downloads, where the game's update is started.
    Update(GameId),
    /// "Open in Steam": show the game in Steam's library (Properties: the Proton choice).
    OpenInSteam(GameId),
    /// Set the game's launch options — Steam's own setting in the account's
    /// `localconfig.vdf`, the one value EasyPlay writes there (ADR-0031 §6a) — only while
    /// Steam is closed (`EP-STM-008` otherwise). Answered, whatever the outcome, with
    /// `QueryResult::Steam` to `reply_to` (the value as it now is); a refusal also emits a
    /// failure, a change `Notice::SteamLaunchOptionsSaved`.
    SetLaunchOptions {
        /// The game.
        id: GameId,
        /// The whole text (`%command%` stands for the game; empty = none).
        options: String,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// "Close Steam": ask the running client of the game's installation to close itself
    /// (`steam -shutdown`), so the launch options can be changed. Never a signal.
    CloseSteam(GameId),
    /// Show the game's Windows storage (`compatdata/<appid>/pfx`) in the file manager.
    OpenPrefixFolder(GameId),
    /// Run a tool in the game's Windows storage with the Proton build Steam last used for
    /// it, inside EasyPlay's protection, only while the game is not running (ADR-0031 §6).
    /// `Tool::Winecfg` is "Windows settings"; `Tool::RunExe` "Run another program".
    RunInPrefix {
        /// The game.
        id: GameId,
        /// The tool.
        tool: crate::model::Tool,
    },
    /// What Steam says about the game, with the notes kept here; answered with
    /// `QueryResult::Steam`.
    Details {
        /// The game.
        id: GameId,
        /// Reply ID.
        reply_to: QueryId,
    },
}

/// What to do with a damaged library (PERSISTENCE §8, the `EP-DB-001` choices).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DatabaseChoice {
    /// Restore this backup (`backups/…`).
    RestoreBackup(PathBuf),
    /// Use what salvage recovered.
    UseRepaired,
    /// Start with an empty library (prefix directories are kept).
    StartEmpty,
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
    /// What a game's files tell (ADR-0028); answered with `QueryResult::About`.
    AboutGame {
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
    /// Check again which games' main programs are there (LOGIC_SPECS §9).
    RescanMissing,
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
    /// The artwork sheet asks which Steam app the game is (ADR-0024 §1); answered with
    /// `QueryResult::SteamApps`. Without `term`, what the computer says (no network) and,
    /// when nothing does, a store search for the game name; with `term`, a store search.
    FindSteamApp {
        /// The game.
        id: GameId,
        /// Words typed in the sheet.
        term: Option<String>,
        /// Reply ID.
        reply_to: QueryId,
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
    /// Install a build or runtime in a Provision session (a channel: check for updates).
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
    /// Remove a store build (refused for external, newest or used builds).
    RemoveRuntime {
        /// The build.
        name: RuntimeName,
    },
    /// Rescan compatibility layers.
    RefreshRuntimes,
    /// Audit every installed runtime now; a modified one is quarantined.
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
    // ---- Developer Mode and diagnostics (DEVELOPER_MODE, ERROR_HANDLING §6–7)
    /// Inspector data; answered with `QueryResult::Inspector`.
    Inspect {
        /// What.
        target: InspectTarget,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// A game's recent sessions (newest first, all kinds); `QueryResult::Sessions`.
    ListSessions {
        /// The game.
        game: GameId,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Read a chunk of a session log file from `offset`; `QueryResult::LogChunk`.
    ReadLog {
        /// The session.
        session: SessionId,
        /// Which file.
        file: LogFile,
        /// Byte offset.
        offset: u64,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// The processes of an open session now; `QueryResult::Processes`.
    ListProcesses {
        /// The session.
        session: SessionId,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Run every Doctor check (D-01…D-17) on a job; the report arrives in the diagnostics
    /// snapshot.
    RunDoctor,
    /// Collect the diagnostics report's files for the preview; `QueryResult::Report`.
    BuildReport {
        /// The game whose settings and session are included.
        game: Option<GameId>,
        /// The session whose logs are included (default: the game's latest).
        session: Option<SessionId>,
        /// Reply ID.
        reply_to: QueryId,
    },
    /// Write the previewed files as a `.tar.gz` to `dest` (chosen in the save dialog).
    SaveReport {
        /// The files, as previewed.
        files: Vec<crate::diagnostics::report::ReportFile>,
        /// Destination.
        dest: PathBuf,
    },
    /// The crash banner was dismissed.
    DismissCrash,
    /// Answer the damaged-library choice (recovery mode).
    ResolveDatabase(DatabaseChoice),
    // ---- Steam games (ADR-0031)
    /// A Steam action.
    Steam(SteamCommand),
    // ---- Chat (ADR-0033)
    /// A Chat action.
    Chat(ChatCommand),
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
            Command::AboutGame { .. } => "about_game",
            Command::QueryStats { .. } => "query_stats",
            Command::RescanMissing => "rescan_missing",
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
            Command::FindSteamApp { .. } => "find_steam_app",
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
            Command::Inspect { .. } => "inspect",
            Command::ListSessions { .. } => "list_sessions",
            Command::ReadLog { .. } => "read_log",
            Command::ListProcesses { .. } => "list_processes",
            Command::RunDoctor => "run_doctor",
            Command::BuildReport { .. } => "build_report",
            Command::SaveReport { .. } => "save_report",
            Command::DismissCrash => "dismiss_crash",
            Command::ResolveDatabase(_) => "resolve_database",
            Command::Steam(_) => "steam",
            Command::Chat(_) => "chat",
            Command::Shutdown => "shutdown",
        }
    }
}
