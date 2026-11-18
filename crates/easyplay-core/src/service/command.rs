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

/// A shared game's height at most: `chat.share_quality`'s values (ADR-0036).
pub use crate::config::schema::ShareQuality as ShareHeight;
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
    Runtime { reference: RuntimeRef, follow: bool },
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

/// What a picture from the clipboard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF.
    Gif,
}

/// Chat (ADR-0033). Answers arrive as `Event::ChatChanged` (the snapshot),
/// `Notice::ChatMessages` / `Notice::ChatMessagesUpdated`, and `QueryResult::Chat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatCommand {
    /// Turn Chat on (with the name friends see, for a first start) or off
    /// (`chat.enabled`, `chat.name`).
    SetEnabled {
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
        chat: crate::model::ChatRef,
        text: String,
        /// The message it answers.
        reply_to: Option<i64>,
        /// Members named with @ (name as written, member number).
        mentions: Vec<(String, i64)>,
    },
    /// Send a voice message recorded in the composer (the preview plays
    /// it, the real service ignores it until recording is carried).
    SendVoice {
        chat: crate::model::ChatRef,
        /// Its length in seconds (1 to 300).
        secs: u32,
    },
    /// Send a picture or GIF file. The service copies it into the Chat folder
    /// (the only folder the engine's sandbox sees) and sends the copy; `.gif` goes as a GIF,
    /// anything else as a picture. Files over `chat::MAX_FILE` (1 MiB, what friends receive
    /// without asking) are refused. Gated by `ChatFeatures::files` (and `gifs` for GIFs).
    SendFile {
        chat: crate::model::ChatRef,
        /// The file on this computer.
        path: PathBuf,
        /// A caption (empty = none).
        text: String,
    },
    /// Fetch a file a friend offered.
    ReceiveFile {
        chat: crate::model::ChatRef,
        item: i64,
    },
    /// Stop sending or fetching a file.
    CancelFile {
        chat: crate::model::ChatRef,
        item: i64,
    },
    /// Open a received or sent file with the desktop's default application.
    OpenFile {
        chat: crate::model::ChatRef,
        item: i64,
    },
    /// Save a copy of a received or sent file at `to` (the user chose it in a save dialog).
    /// Only a file inside the Chat folder is copied; the copy happens off the service thread.
    SaveFile {
        chat: crate::model::ChatRef,
        item: i64,
        to: PathBuf,
    },
    /// Send a picture from the clipboard: the service writes it into the Chat folder and
    /// sends it like `SendFile`.
    SendClipboard {
        chat: crate::model::ChatRef,
        bytes: Vec<u8>,
        kind: ClipKind,
        /// A caption (empty = none).
        text: String,
    },
    /// Make a group's invite link (`ChatGroup::link`).
    CreateGroupLink(i64),
    /// Delete it; links already shared stop working.
    DeleteGroupLink(i64),
    /// Change the text of one of the user's messages.
    Edit {
        chat: crate::model::ChatRef,
        item: i64,
        text: String,
    },
    /// Delete one of the user's messages for everyone.
    Delete {
        chat: crate::model::ChatRef,
        item: i64,
    },
    /// Add or take back a reaction.
    React {
        chat: crate::model::ChatRef,
        item: i64,
        emoji: String,
        /// Add (or remove).
        on: bool,
    },
    /// Mute or unmute a conversation's notifications.
    SetMuted {
        chat: crate::model::ChatRef,
        on: bool,
    },
    /// Block or unblock a friend (they are not told).
    SetBlocked { friend: i64, on: bool },
    /// Block or unblock a group member (their messages collapse).
    SetMemberBlocked { group: i64, member: i64, on: bool },
    /// Fetch a friend's security code into `ChatSnapshot::code`.
    ShowCode(i64),
    /// The codes matched: mark the friend verified.
    Verify {
        friend: i64,
        /// The code that was compared.
        code: String,
    },
    /// The user's profile (name, about, avatar pair; the picture is kept).
    SetProfile(crate::model::ChatProfile),
    /// Make a picture file the user's profile picture (ADR-0038): read, cropped to `crop`
    /// (`None` = the centre square) and shrunk off the service thread. Files over
    /// `chat::picture::MAX_SOURCE` (20 MiB) are refused (`EP-CHT-009`), as is anything that is
    /// not a picture (`EP-CHT-008`). The new picture arrives in `ChatProfile::picture`.
    SetPicture {
        path: PathBuf,
        crop: Option<crate::model::PictureCrop>,
    },
    /// Remove the user's profile picture.
    ClearPicture,
    /// Read a picture file for the crop step, answered with `QueryResult::PictureSource` (or
    /// `Failed`: `EP-CHT-008`, `EP-CHT-009`) to `reply_to`. The page does no file I/O.
    LoadPicture { path: PathBuf, reply_to: QueryId },
    /// Make a picture file a group's picture (owners only: `ChatGroup::owner`), as
    /// `SetPicture`.
    SetGroupPicture {
        group: i64,
        path: PathBuf,
        crop: Option<crate::model::PictureCrop>,
    },
    /// Remove a group's picture (owners only).
    ClearGroupPicture(i64),
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
    /// Start or stop the microphone test (Settings › Chat, "Let's check"; ADR-0035): the
    /// chosen microphone with `chat.mic_volume` and noise suppression, its level in
    /// `ChatLive::mic_test`, played back on the chosen speakers half a second late. A call
    /// starting stops it; the page stops it when Settings closes.
    MicTest(bool),
    /// List the microphones and speakers again into `ChatSnapshot::audio_devices` (when
    /// Settings opens).
    RefreshAudioDevices,
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
    /// The shared game's picture: its height at most and frames a second (30 or 60). Applies
    /// to the running share at once and is kept for the next one (`chat.share_quality`,
    /// `chat.share_fps`; ADR-0036).
    SetShareQuality { height: ShareHeight, fps: u32 },
    /// Open a link from a message in the browser; with `chat.check_links` on, after the
    /// site is checked (a harmful one comes back as `Notice::ChatUnsafeLink` instead).
    OpenLink(String),
    /// Open a link without the check: the user chose "Open anyway" after
    /// `Notice::ChatUnsafeLink` (only `http` and `https`).
    OpenLinkAnyway(String),
    /// Search Giphy for GIFs (only the words are sent; `crate::gifs`), answered with
    /// `QueryResult::Gifs` to `reply_to`. Nothing is sent while `chat.gif_search` is off or
    /// no key is known (the answer says why).
    GifSearch { query: String, reply_to: QueryId },
    /// Giphy's trending GIFs, answered with `QueryResult::Gifs` (same rules as search).
    GifTrending { reply_to: QueryId },
    /// A picker picture from Giphy's media hosts, answered with `QueryResult::GifPreview`.
    GifPreview {
        /// `crate::gifs::Gif::preview_url`.
        url: String,
        reply_to: QueryId,
    },
    /// Send a GIF: one from Giphy is downloaded first (at most `crate::gifs::MAX_SEND`) and
    /// sent as a file (`SendFile`), so friends never contact Giphy. A failure comes back as
    /// `Notice::ChatGifNotSent`.
    SendGif {
        chat: crate::model::ChatRef,
        gif: crate::gifs::GifPick,
    },
    /// Everything in the conversation was seen.
    MarkRead(crate::model::ChatRef),
    /// Messages of a conversation, answered with `QueryResult::Chat` to `reply_to`.
    LoadChat {
        chat: crate::model::ChatRef,
        /// Only older than this message (`None` = the latest).
        before: Option<i64>,
        reply_to: QueryId,
    },
    /// Make a group.
    NewGroup(String),
    /// Invite a friend into a group.
    AddMember { group: i64, friend: i64 },
    /// Join a group the user was invited to.
    JoinGroup(i64),
    /// Leave a group.
    LeaveGroup(i64),
    /// Remove a friend and the conversation.
    RemoveFriend(i64),
}

/// What the overlay hotkey asks (ADR-0037).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayAction {
    Toggle,
    Open,
    Close,
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
        id: GameId,
        /// The whole text (`%command%` stands for the game; empty = none).
        options: String,
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
        id: GameId,
        tool: crate::model::Tool,
    },
    /// What Steam says about the game, with the notes kept here; answered with
    /// `QueryResult::Steam`.
    Details { id: GameId, reply_to: QueryId },
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

/// A command for the AppService. The log name is the variant in snake_case.
#[derive(Clone, Debug, PartialEq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Command {
    // ---- entry points
    /// Open up to 64 paths (LOGIC_SPECS §1).
    OpenPaths {
        /// The paths, in order.
        paths: Vec<PathBuf>,
        origin: OpenOrigin,
    },
    // ---- library
    /// Add a game from a confirmed setup draft (LOGIC_SPECS §2).
    CreateGame {
        /// The draft as edited in the setup dialog.
        draft: Box<GameDraft>,
        then: AfterCreate,
    },
    /// The setup dialog was closed without adding (the next queued draft is shown).
    DiscardDraft,
    /// Remove a game (LOGIC_SPECS §11).
    RemoveGame { id: GameId, options: RemoveOptions },
    /// Favourite on/off (idempotent).
    SetFavorite { id: GameId, favorite: bool },
    /// Replace a game's tags.
    SetTags {
        id: GameId,
        /// The tags (unknown ones are created).
        tags: Vec<TagName>,
    },
    /// Hidden on/off.
    SetHidden { id: GameId, hidden: bool },
    /// Change a game (validated as a whole; `Event::Failure` on a refused change).
    UpdateGame { id: GameId, edits: Vec<GameEdit> },
    /// Load a game's full record; answered with `Event::QueryResult`.
    LoadGame { id: GameId, reply_to: QueryId },
    /// What a game's files tell (ADR-0028); answered with `QueryResult::About`.
    AboutGame { id: GameId, reply_to: QueryId },
    /// A statistics query; answered with `Event::QueryResult`.
    QueryStats {
        request: StatsRequest,
        reply_to: QueryId,
    },
    /// Check again which games' main programs are there (LOGIC_SPECS §9).
    RescanMissing,
    /// Delete all play history (STATISTICS §9).
    ClearStatistics,
    /// Delete one game's play history.
    ResetGameStatistics { id: GameId },
    /// Delete one ended session from history.
    RemoveSession { id: SessionId },
    /// Add a program to a game (`AddExecutable`).
    AddExecutable {
        id: GameId,
        path: PathBuf,
        role: crate::model::ExeRole,
    },
    /// Point a moved program at its new place (*Locate…*, LOGIC_SPECS §8). `exe` defaults
    /// to the primary.
    LocateExecutable {
        id: GameId,
        exe: Option<ExecutableId>,
        new_path: PathBuf,
    },
    /// List the programs in a folder (GAME_LIBRARY §11); answered with `QueryResult::Folder`.
    ScanFolder { folder: PathBuf, reply_to: QueryId },
    /// Measure the private Windows storage; answered with `QueryResult::PrefixSize`.
    MeasurePrefix { id: GameId, reply_to: QueryId },
    // ---- artwork
    /// Import artwork for one slot. `ArtSource::SteamCdn(0)` means "find the Steam app ID"
    /// (the chain of ADR-0018; only with `artwork.online = "steam"`).
    ImportArtwork {
        id: GameId,
        slot: ArtSlot,
        source: ArtSource,
    },
    /// Fetch every slot from the Steam CDN on request ("Find artwork online").
    FetchOnlineArtwork { id: GameId },
    /// The artwork sheet asks which Steam app the game is (ADR-0024 §1); answered with
    /// `QueryResult::SteamApps`. Without `term`, what the computer says (no network) and,
    /// when nothing does, a store search for the game name; with `term`, a store search.
    FindSteamApp {
        id: GameId,
        /// Words typed in the sheet.
        term: Option<String>,
        reply_to: QueryId,
    },
    // ---- launching
    /// Launch a game (LOGIC_SPECS §3).
    Launch {
        id: GameId,
        /// Which program (default: primary).
        exe: Option<ExecutableId>,
        /// Where it was requested.
        origin: LaunchOrigin,
    },
    /// Cancel a launch before its game starts (LOGIC_SPECS §3 "Cancellation"); from
    /// `Starting` on it stops the session.
    CancelLaunch { id: GameId },
    /// Run a tool in the game's private storage (SVC-005, a `Tool` session).
    RunTool {
        id: GameId,
        tool: crate::model::Tool,
    },
    /// Stop a session (always accepted, idempotent; LOGIC_SPECS §6).
    StopSession { session: SessionId, mode: StopMode },
    /// Answer a prompt raised by a parked flow.
    AnswerPrompt {
        prompt: PromptId,
        answer: PromptAnswer,
    },
    // ---- prefix
    /// Reset the private Windows storage (LOGIC_SPECS §12).
    ResetPrefix { id: GameId, options: ResetOptions },
    // ---- runtimes
    /// Install a build or runtime in a Provision session (a channel: check for updates).
    ProvisionRuntime { target: RuntimeRef },
    /// Copy a Steam Linux Runtime variant that umu installed for the user into the store
    /// (`runtime::adopt`). Never writes outside the store.
    AdoptRuntime { variant: SlrVariant },
    /// Remove a store build (refused for external, newest or used builds).
    RemoveRuntime { name: RuntimeName },
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
        id: GameId,
        /// Create (`false`: remove).
        enable: bool,
    },
    /// Open a known folder in the file manager.
    OpenFolder(FolderRequest),
    /// Open a session log (the game's latest when `session` is `None`).
    ViewLog {
        session: Option<SessionId>,
        game: Option<GameId>,
    },
    // ---- Developer Mode and diagnostics (DEVELOPER_MODE, ERROR_HANDLING §6–7)
    /// Inspector data; answered with `QueryResult::Inspector`.
    Inspect {
        target: InspectTarget,
        reply_to: QueryId,
    },
    /// A game's recent sessions (newest first, all kinds); `QueryResult::Sessions`.
    ListSessions { game: GameId, reply_to: QueryId },
    /// Read a chunk of a session log file from `offset`; `QueryResult::LogChunk`.
    ReadLog {
        session: SessionId,
        file: LogFile,
        offset: u64,
        reply_to: QueryId,
    },
    /// The processes of an open session now; `QueryResult::Processes`.
    ListProcesses {
        session: SessionId,
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
        reply_to: QueryId,
    },
    /// Write the previewed files as a `.tar.gz` to `dest` (chosen in the save dialog).
    SaveReport {
        files: Vec<crate::diagnostics::report::ReportFile>,
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
    /// Open or close the in-game overlay (ADR-0037; the hotkey's `easyplay overlay`).
    Overlay(OverlayAction),
    // ---- lifecycle
    /// Stop the service: parked flows are cancelled, running games are left alone.
    Shutdown,
}

impl Command {
    /// A short name for logs.
    pub fn name(&self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::game_root::{GameRootProposal, RootKind, RootReason};
    use crate::model::{ExeRole, PeInfo, Tool};
    use crate::stats::DateRange;

    fn draft() -> GameDraft {
        GameDraft {
            path: PathBuf::from("/g/a.exe"),
            canonical_path: PathBuf::from("/g/a.exe"),
            mount_point: None,
            fingerprint: crate::model::Fingerprint {
                size: 0,
                mtime_ns: 0,
                quick_hash: [0; 32],
                full_hash: None,
            },
            pe: PeInfo::default(),
            imports: Vec::new(),
            name: String::new(),
            name_is_user_set: false,
            proposal: GameRootProposal {
                kind: RootKind::LooseFile,
                root: None,
                candidates: Vec::new(),
                reason: RootReason::ExecutableFolder,
            },
            root: None,
            policy: crate::sandbox::presets::offline(),
            launch: LaunchConfig::default(),
            runtime: RuntimeRef::Default,
            kind: crate::model::EntryKind::Game,
            icon: None,
            hints: Default::default(),
            umu_id: None,
            tags: Vec::new(),
            issues: Vec::new(),
        }
    }

    /// Every command with the name logs have always shown for it.
    fn every_command() -> Vec<(Command, &'static str)> {
        let uuid = uuid::Uuid::nil();
        let (game, session) = (GameId::from_uuid(uuid), SessionId::from_uuid(uuid));
        let (prompt, reply_to) = (PromptId::from_uuid(uuid), QueryId::from_uuid(uuid));
        let path = PathBuf::from("/x");
        let range = DateRange {
            from: None,
            to: jiff::civil::date(2026, 1, 1),
        };
        vec![
            (
                Command::OpenPaths {
                    paths: vec![],
                    origin: OpenOrigin::Cli,
                },
                "open_paths",
            ),
            (
                Command::CreateGame {
                    draft: Box::new(draft()),
                    then: AfterCreate::Nothing,
                },
                "create_game",
            ),
            (Command::DiscardDraft, "discard_draft"),
            (
                Command::RemoveGame {
                    id: game,
                    options: RemoveOptions::default(),
                },
                "remove_game",
            ),
            (
                Command::SetFavorite {
                    id: game,
                    favorite: true,
                },
                "set_favorite",
            ),
            (
                Command::SetTags {
                    id: game,
                    tags: vec![],
                },
                "set_tags",
            ),
            (
                Command::SetHidden {
                    id: game,
                    hidden: true,
                },
                "set_hidden",
            ),
            (
                Command::UpdateGame {
                    id: game,
                    edits: vec![],
                },
                "update_game",
            ),
            (Command::LoadGame { id: game, reply_to }, "load_game"),
            (Command::AboutGame { id: game, reply_to }, "about_game"),
            (
                Command::QueryStats {
                    request: StatsRequest::WeekdayDistribution { range },
                    reply_to,
                },
                "query_stats",
            ),
            (Command::RescanMissing, "rescan_missing"),
            (Command::ClearStatistics, "clear_statistics"),
            (
                Command::ResetGameStatistics { id: game },
                "reset_game_statistics",
            ),
            (Command::RemoveSession { id: session }, "remove_session"),
            (
                Command::AddExecutable {
                    id: game,
                    path: path.clone(),
                    role: ExeRole::Primary,
                },
                "add_executable",
            ),
            (
                Command::LocateExecutable {
                    id: game,
                    exe: None,
                    new_path: path.clone(),
                },
                "locate_executable",
            ),
            (
                Command::ScanFolder {
                    folder: path.clone(),
                    reply_to,
                },
                "scan_folder",
            ),
            (
                Command::MeasurePrefix { id: game, reply_to },
                "measure_prefix",
            ),
            (Command::CancelLaunch { id: game }, "cancel_launch"),
            (
                Command::RunTool {
                    id: game,
                    tool: Tool::Winecfg,
                },
                "run_tool",
            ),
            (
                Command::ImportArtwork {
                    id: game,
                    slot: ArtSlot::Cover,
                    source: ArtSource::Generated,
                },
                "import_artwork",
            ),
            (
                Command::FetchOnlineArtwork { id: game },
                "fetch_online_artwork",
            ),
            (
                Command::FindSteamApp {
                    id: game,
                    term: None,
                    reply_to,
                },
                "find_steam_app",
            ),
            (
                Command::Launch {
                    id: game,
                    exe: None,
                    origin: LaunchOrigin::Library,
                },
                "launch",
            ),
            (
                Command::StopSession {
                    session,
                    mode: StopMode::Graceful,
                },
                "stop_session",
            ),
            (
                Command::AnswerPrompt {
                    prompt,
                    answer: PromptAnswer::Cancel,
                },
                "answer_prompt",
            ),
            (
                Command::ResetPrefix {
                    id: game,
                    options: ResetOptions::default(),
                },
                "reset_prefix",
            ),
            (
                Command::ProvisionRuntime {
                    target: RuntimeRef::Default,
                },
                "provision_runtime",
            ),
            (
                Command::AdoptRuntime {
                    variant: SlrVariant::Steamrt3,
                },
                "adopt_runtime",
            ),
            (
                Command::RemoveRuntime {
                    name: RuntimeName::new("GE-Proton9-1").expect("a valid name"),
                },
                "remove_runtime",
            ),
            (Command::RefreshRuntimes, "refresh_runtimes"),
            (Command::VerifyRuntime, "verify_runtime"),
            (
                Command::SetPreference(PreferencePatch::default()),
                "set_preference",
            ),
            (Command::RefreshHostFacts, "refresh_host_facts"),
            (
                Command::SetDefaultHandler { enable: true },
                "set_default_handler",
            ),
            (
                Command::SetShortcut {
                    id: game,
                    enable: true,
                },
                "set_shortcut",
            ),
            (
                Command::OpenFolder(FolderRequest::DataFolder),
                "open_folder",
            ),
            (
                Command::ViewLog {
                    session: None,
                    game: None,
                },
                "view_log",
            ),
            (
                Command::Inspect {
                    target: InspectTarget::Session(session),
                    reply_to,
                },
                "inspect",
            ),
            (Command::ListSessions { game, reply_to }, "list_sessions"),
            (
                Command::ReadLog {
                    session,
                    file: LogFile::Game,
                    offset: 0,
                    reply_to,
                },
                "read_log",
            ),
            (
                Command::ListProcesses { session, reply_to },
                "list_processes",
            ),
            (Command::RunDoctor, "run_doctor"),
            (
                Command::BuildReport {
                    game: None,
                    session: None,
                    reply_to,
                },
                "build_report",
            ),
            (
                Command::SaveReport {
                    files: vec![],
                    dest: path,
                },
                "save_report",
            ),
            (Command::DismissCrash, "dismiss_crash"),
            (
                Command::ResolveDatabase(DatabaseChoice::UseRepaired),
                "resolve_database",
            ),
            (Command::Steam(SteamCommand::Rescan), "steam"),
            (Command::Chat(ChatCommand::Connect(String::new())), "chat"),
            (Command::Shutdown, "shutdown"),
        ]
    }

    #[test]
    fn every_command_keeps_the_log_name_it_always_had() {
        let all = every_command();
        assert_eq!(all.len(), 51, "a command was added: list its log name here");
        for (cmd, name) in &all {
            assert_eq!(cmd.name(), *name);
        }
    }
}
