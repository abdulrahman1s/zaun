//! Games and their library summaries (DATA_MODEL §2, GAME_LIBRARY §1).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use smallvec::SmallVec;

use super::{
    ExeRole, Executable, LaunchConfig, NetworkAccess, Preset, Problem, RuntimeRef, SandboxPolicy,
    Sha256, ValidationError,
};
use crate::clock::Timestamp;
use crate::consts::{
    GAME_NAME_MAX_CHARS, GAME_NOTES_MAX_CHARS, MAX_EXECUTABLES_PER_GAME, MAX_UMU_ID_BYTES,
};
use crate::ids::{GameId, RuntimeName, TagId};

/// A configured game with its executables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    /// Identifier.
    pub id: GameId,
    /// Display name, 1–80 characters.
    pub name: String,
    /// Sort key derived from the name.
    pub sort_name: String,
    /// Whether the user edited the name (it is then never replaced automatically).
    pub name_is_user_set: bool,
    /// The game folder; `None` for a loose executable.
    pub root: Option<PathBuf>,
    /// When it was added.
    pub added_at: Timestamp,
    /// Last change.
    pub updated_at: Timestamp,
    /// Favourite.
    pub favorite: bool,
    /// Hidden from the main collections.
    pub hidden: bool,
    /// User notes, ≤ 4000 characters.
    pub notes: String,
    /// Sandbox policy.
    pub policy: SandboxPolicy,
    /// When the policy last changed.
    pub policy_updated_at: Timestamp,
    /// Launch configuration.
    pub launch: LaunchConfig,
    /// umu `GAMEID`, `^[A-Za-z0-9._-]{1,64}$`.
    pub umu_id: Option<String>,
    /// umu `STORE`.
    pub store: Option<Store>,
    /// Compatibility layer.
    pub runtime: RuntimeRef,
    /// Whether a channel reference follows new builds.
    pub runtime_follow_updates: bool,
    /// The build the reference last resolved to.
    pub runtime_last_resolved: Option<RuntimeName>,
    /// First untrusted launch in the current prefix (ADR-0014).
    pub prefix_tainted_at: Option<Timestamp>,
    /// Build the prefix was prepared with.
    pub prefix_prepared_with: Option<RuntimeName>,
    /// Flags.
    pub flags: GameFlags,
    /// Last launch.
    pub last_launched_at: Option<Timestamp>,
    /// Steam app ID used to fetch artwork from the Steam CDN (ADR-0018); `None` = unknown.
    pub steam_app_id: Option<u32>,
    /// Whether the user chose the Steam app ID (it is then never replaced automatically).
    pub steam_app_id_is_user_set: bool,
    /// Executables; exactly one has role `Primary`.
    pub executables: Vec<Executable>,
}

impl Game {
    /// The primary executable.
    pub fn primary(&self) -> Option<&Executable> {
        self.executables.iter().find(|e| e.role == ExeRole::Primary)
    }

    /// Whether the prefix has run untrusted code.
    pub fn is_prefix_tainted(&self) -> bool {
        self.prefix_tainted_at.is_some()
    }

    /// Check the field limits of DATA_MODEL §2 and CONFIGURATION §3 and of every nested
    /// document.
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_name(&self.name)?;
        validate_notes(&self.notes)?;
        if let Some(id) = &self.umu_id {
            validate_umu_id(id)?;
        }
        if let Some(root) = &self.root
            && !root.is_absolute()
        {
            return Err(ValidationError::new("game.root", Problem::NotAbsolute));
        }
        if self.executables.len() > MAX_EXECUTABLES_PER_GAME {
            return Err(ValidationError::new(
                "game.executables",
                Problem::TooMany {
                    max: MAX_EXECUTABLES_PER_GAME,
                },
            ));
        }
        let primaries = self
            .executables
            .iter()
            .filter(|e| e.role == ExeRole::Primary)
            .count();
        if primaries != 1 {
            return Err(ValidationError::new(
                "game.executables",
                Problem::ExactlyOnePrimary { found: primaries },
            ));
        }
        if self.steam_app_id == Some(0) {
            return Err(ValidationError::new(
                "game.steam_app_id",
                Problem::OutOfRange,
            ));
        }
        for exe in &self.executables {
            exe.validate()?;
        }
        self.policy.validate()?;
        self.launch.validate()?;
        if let Some(Store::Other(s)) = &self.store
            && !is_umu_token(s, MAX_UMU_ID_BYTES)
        {
            return Err(ValidationError::new(
                "game.store",
                Problem::InvalidCharacters,
            ));
        }
        Ok(())
    }
}

/// A game name: trimmed length 1–80 characters, no control characters.
pub fn validate_name(name: &str) -> Result<(), ValidationError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::new("game.name", Problem::Empty));
    }
    if name.chars().count() > GAME_NAME_MAX_CHARS {
        return Err(ValidationError::new(
            "game.name",
            Problem::TooLong {
                max: GAME_NAME_MAX_CHARS,
            },
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(ValidationError::new(
            "game.name",
            Problem::InvalidCharacters,
        ));
    }
    Ok(())
}

/// Notes: at most 4000 characters; newlines and tabs allowed, other control characters not.
pub fn validate_notes(notes: &str) -> Result<(), ValidationError> {
    if notes.chars().count() > GAME_NOTES_MAX_CHARS {
        return Err(ValidationError::new(
            "game.notes",
            Problem::TooLong {
                max: GAME_NOTES_MAX_CHARS,
            },
        ));
    }
    if notes
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
    {
        return Err(ValidationError::new(
            "game.notes",
            Problem::InvalidCharacters,
        ));
    }
    Ok(())
}

/// umu `GAMEID`: `^[A-Za-z0-9._-]{1,64}$`.
pub fn validate_umu_id(id: &str) -> Result<(), ValidationError> {
    if is_umu_token(id, MAX_UMU_ID_BYTES) {
        Ok(())
    } else {
        Err(ValidationError::new("game.umu_id", Problem::InvalidFormat))
    }
}

fn is_umu_token(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Derive the sort key: lower case, leading English articles moved out of the way.
pub fn sort_name_of(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article)
            && !rest.trim().is_empty()
        {
            return rest.trim_start().to_owned();
        }
    }
    lower
}

bitflags::bitflags! {
    /// Per-game flags (`games.flags`). Unknown bits are preserved.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct GameFlags: u32 {
        /// A runtime audit found changes after this game ran (RUNTIME_MANAGEMENT §6).
        const SUSPECTED_TAMPER = 1 << 0;
        /// The user acknowledged the X11 notice for this game.
        const X11_NOTICE_ACK = 1 << 1;
        /// The user acknowledged a broad-grant warning.
        const BROAD_GRANT_ACK = 1 << 2;
    }
}

impl Serialize for GameFlags {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(self.bits())
    }
}

impl<'de> Deserialize<'de> for GameFlags {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(GameFlags::from_bits_retain(u32::deserialize(d)?))
    }
}

/// The store a game comes from, for umu's `STORE` (game fixes).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    /// Epic Games Store.
    Egs,
    /// GOG.
    Gog,
    /// Battle.net.
    Battlenet,
    /// Amazon Games.
    Amazon,
    /// Humble.
    Humble,
    /// itch.io.
    Itchio,
    /// Ubisoft Connect.
    Ubisoft,
    /// EA app.
    Ea,
    /// ZOOM Platform.
    Zoom,
    /// Another umu store identifier (`[A-Za-z0-9._-]{1,64}`).
    Other(String),
}

impl Store {
    /// The value of umu's `STORE` variable and of the `games.store` column.
    pub fn as_str(&self) -> &str {
        match self {
            Store::Egs => "egs",
            Store::Gog => "gog",
            Store::Battlenet => "battlenet",
            Store::Amazon => "amazon",
            Store::Humble => "humble",
            Store::Itchio => "itchio",
            Store::Ubisoft => "ubisoft",
            Store::Ea => "ea",
            Store::Zoom => "zoomplatform",
            Store::Other(s) => s,
        }
    }

    /// Parse [`Store::as_str`].
    pub fn parse(s: &str) -> Store {
        match s {
            "egs" => Store::Egs,
            "gog" => Store::Gog,
            "battlenet" => Store::Battlenet,
            "amazon" => Store::Amazon,
            "humble" => Store::Humble,
            "itchio" => Store::Itchio,
            "ubisoft" => Store::Ubisoft,
            "ea" => Store::Ea,
            "zoomplatform" => Store::Zoom,
            other => Store::Other(other.to_owned()),
        }
    }
}

// ---- Library summary (GAME_LIBRARY §1.1) ------------------------------------------------

/// Content hash of an artwork file; the texture-cache key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtKey(#[serde(with = "super::executable::hex32")] pub Sha256);

/// Whether a game's files are reachable (LOGIC_SPECS §9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// The primary executable exists.
    Present,
    /// It does not.
    Missing,
    /// The drive it is on is not mounted.
    DriveDisconnected,
    /// It exists but cannot be read.
    Unreadable,
    /// Not checked yet.
    #[default]
    Unknown,
}

/// Coarse launch state shown on cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStateTag {
    /// Validating.
    Validating,
    /// Resolving the compatibility layer.
    ResolvingRuntime,
    /// Downloading a compatibility layer.
    Provisioning,
    /// Preparing the prefix.
    Preparing,
    /// Waiting for a runtime audit.
    AuditingRuntime,
    /// Planning.
    Planning,
    /// Starting the supervisor.
    Spawning,
    /// The session is starting.
    Starting,
}

/// What a game is doing right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Nothing.
    #[default]
    Idle,
    /// A launch is in progress.
    Launching(LaunchStateTag),
    /// A session is running.
    Running {
        /// Session start.
        since: Timestamp,
    },
    /// The main program exited but other processes remain.
    Lingering,
    /// A stop was requested.
    Stopping,
}

/// What kind of library entry this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A game.
    Game,
    /// An installer not yet converted.
    Installer,
    /// A tool added as an entry.
    Tool,
}

/// Where save data lives, for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveLocationKind {
    /// Inside the prefix.
    Internal,
    /// A user-chosen folder.
    External,
}

/// Summary flags derived for display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SummaryFlags {
    /// [`GameFlags::SUSPECTED_TAMPER`].
    pub suspected_tamper: bool,
    /// Something needs the user's attention (missing grant, invalid runtime…).
    pub needs_attention: bool,
    /// A non-blocking warning exists.
    pub has_warning: bool,
}

/// Everything the library screens show about one game, cheap to clone and share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameSummary {
    /// Identifier.
    pub id: GameId,
    /// Name.
    pub name: Arc<str>,
    /// Sort key.
    pub sort_name: Arc<str>,
    /// Cover art.
    pub cover: Option<ArtKey>,
    /// Icon.
    pub icon: Option<ArtKey>,
    /// Favourite.
    pub favorite: bool,
    /// Hidden.
    pub hidden: bool,
    /// Tags.
    pub tags: SmallVec<[TagId; 4]>,
    /// When it was added.
    pub added_at: Timestamp,
    /// Last played.
    pub last_played_at: Option<Timestamp>,
    /// First played.
    pub first_played_at: Option<Timestamp>,
    /// Total counted playtime.
    pub total_playtime_ms: u64,
    /// Counted sessions.
    pub session_count: u32,
    /// Launches.
    pub launch_count: u32,
    /// Preset.
    pub preset: Preset,
    /// Network access.
    pub network: NetworkAccess,
    /// Compatibility-layer label, e.g. "GE-Proton 10-25".
    pub runtime_label: Arc<str>,
    /// Primary executable path.
    pub primary_exe_path: Arc<Path>,
    /// Game folder.
    pub root: Option<Arc<Path>>,
    /// Save location.
    pub save_location: SaveLocationKind,
    /// Entry kind.
    pub kind: EntryKind,
    /// Availability.
    pub availability: Availability,
    /// Activity.
    pub activity: Activity,
    /// Flags.
    pub flags: SummaryFlags,
}
