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
            Command::Shutdown => "shutdown",
        }
    }
}
