//! Repositories: one trait per aggregate, implemented once for every [`Store`] (DATA_MODEL
//! §6, MODULES §4).
//!
//! Row structs (deriving `ormlite::Model` where the table has a single-column key) are
//! private to the files of this module and are converted to and from `model` types here.
//! Methods that write several rows are atomic; inside a [`Tx`](super::Tx) they nest as
//! savepoints. Repositories enforce constraints and count limits, never business rules.

mod artwork;
mod executables;
mod games;
mod kv;
mod runtimes;
mod sessions;
mod stats;
mod tags;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::path_codec::{self, PathCodecError};
use super::{StorageError, StorageResult, Store};
use crate::clock::Timestamp;
use crate::ids::{ExecutableId, GameId, RuntimeName, SessionId, TagId, TagName};
use crate::model::{
    ArtSlot, ArtworkRef, EndReason, Executable, Fingerprint, Game, GameFlags, GameKey, GameStats,
    LaunchConfig, PlaytimeDay, RuntimeBuild, RuntimeRef, SandboxPolicy, Session, SessionKind,
    SessionState, Sha256, SlrVariant, Store as GameStore, Tag, TagColor,
};

// ---- Public record types -----------------------------------------------------------------

/// A stored game as read from the database. The JSON documents are kept as results so that
/// one unreadable document disables only that game (LOGIC_SPECS §15).
#[derive(Clone, Debug, PartialEq)]
pub struct GameRecord {
    /// Identifier.
    pub id: GameId,
    /// Display name.
    pub name: String,
    /// Sort key.
    pub sort_name: String,
    /// Whether the user edited the name.
    pub name_is_user_set: bool,
    /// Game folder.
    pub root: Option<PathBuf>,
    /// When it was added.
    pub added_at: Timestamp,
    /// Last change.
    pub updated_at: Timestamp,
    /// Soft-delete marker (LOGIC_SPECS §11 "Detach").
    pub deleted_at: Option<Timestamp>,
    /// Favourite.
    pub favorite: bool,
    /// Hidden.
    pub hidden: bool,
    /// Notes.
    pub notes: String,
    /// The policy document, upgraded in memory if it was older.
    pub policy: Result<SandboxPolicy, crate::model::DocError>,
    /// When the policy last changed.
    pub policy_updated_at: Timestamp,
    /// The launch document, upgraded in memory if it was older.
    pub launch: Result<LaunchConfig, crate::model::DocError>,
    /// umu `GAMEID`.
    pub umu_id: Option<String>,
    /// umu `STORE`.
    pub store: Option<GameStore>,
    /// Compatibility layer.
    pub runtime: RuntimeRef,
    /// Whether a channel reference follows new builds.
    pub runtime_follow_updates: bool,
    /// Last resolution.
    pub runtime_last_resolved: Option<RuntimeName>,
    /// Prefix taint time.
    pub prefix_tainted_at: Option<Timestamp>,
    /// Build the prefix was prepared with.
    pub prefix_prepared_with: Option<RuntimeName>,
    /// Flags.
    pub flags: GameFlags,
    /// Last launch.
    pub last_launched_at: Option<Timestamp>,
    /// Steam app ID for artwork (ADR-0018).
    pub steam_app_id: Option<u32>,
    /// Whether the user chose the Steam app ID.
    pub steam_app_id_is_user_set: bool,
    /// Executables (primary first, then by `added_at`).
    pub executables: Vec<Executable>,
}

impl GameRecord {
    /// Whether both documents are readable.
    pub fn is_readable(&self) -> bool {
        self.policy.is_ok() && self.launch.is_ok()
    }

    /// The domain value; fails with [`StorageError::Document`] when a document is unreadable.
    pub fn into_game(self) -> StorageResult<Game> {
        let policy = self.policy.map_err(|error| StorageError::Document {
            game: self.id,
            doc: super::DocKind::Policy,
            error,
        })?;
        let launch = self.launch.map_err(|error| StorageError::Document {
            game: self.id,
            doc: super::DocKind::Launch,
            error,
        })?;
        Ok(Game {
            id: self.id,
            name: self.name,
            sort_name: self.sort_name,
            name_is_user_set: self.name_is_user_set,
            root: self.root,
            added_at: self.added_at,
            updated_at: self.updated_at,
            favorite: self.favorite,
            hidden: self.hidden,
            notes: self.notes,
            policy,
            policy_updated_at: self.policy_updated_at,
            launch,
            umu_id: self.umu_id,
            store: self.store,
            runtime: self.runtime,
            runtime_follow_updates: self.runtime_follow_updates,
            runtime_last_resolved: self.runtime_last_resolved,
            prefix_tainted_at: self.prefix_tainted_at,
            prefix_prepared_with: self.prefix_prepared_with,
            flags: self.flags,
            last_launched_at: self.last_launched_at,
            steam_app_id: self.steam_app_id,
            steam_app_id_is_user_set: self.steam_app_id_is_user_set,
            executables: self.executables,
        })
    }
}

/// Everything the library index is built from: five sequential scans joined in memory
/// (DATA_MODEL §6.2), read in one snapshot. Soft-deleted games are excluded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibraryRows {
    /// Games ordered by `sort_name`, executables attached.
    pub games: Vec<GameRecord>,
    /// All tags by name.
    pub tags: Vec<Tag>,
    /// `game_tags` links.
    pub game_tags: Vec<(GameId, TagId)>,
    /// Artwork rows.
    pub artwork: Vec<ArtworkRef>,
    /// `game_stats` rows.
    pub stats: Vec<GameStats>,
}

/// A partial update of a game's own columns. `None` leaves a field unchanged; for optional
/// columns `Some(None)` clears it. `updated_at` is always set; `policy_updated_at` when the
/// policy changes; `sort_name` follows `name` unless given explicitly.
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(clippy::option_option)]
pub struct GamePatch {
    /// Name.
    pub name: Option<String>,
    /// Sort key (derived from `name` when `None` and `name` is set).
    pub sort_name: Option<String>,
    /// Name edited by the user.
    pub name_is_user_set: Option<bool>,
    /// Game folder.
    pub root: Option<Option<PathBuf>>,
    /// Favourite.
    pub favorite: Option<bool>,
    /// Hidden.
    pub hidden: Option<bool>,
    /// Notes.
    pub notes: Option<String>,
    /// Policy.
    pub policy: Option<SandboxPolicy>,
    /// Launch configuration.
    pub launch: Option<LaunchConfig>,
    /// umu `GAMEID`.
    pub umu_id: Option<Option<String>>,
    /// umu `STORE`.
    pub store: Option<Option<GameStore>>,
    /// Compatibility layer.
    pub runtime: Option<RuntimeRef>,
    /// Follow updates.
    pub runtime_follow_updates: Option<bool>,
    /// Last resolution.
    pub runtime_last_resolved: Option<Option<RuntimeName>>,
    /// Build the prefix was prepared with.
    pub prefix_prepared_with: Option<Option<RuntimeName>>,
    /// Flags.
    pub flags: Option<GameFlags>,
    /// Steam app ID for artwork (`Some(None)` clears it).
    pub steam_app_id: Option<Option<u32>>,
    /// Whether the user chose the Steam app ID.
    pub steam_app_id_is_user_set: Option<bool>,
}

/// A new session row (written by the supervisor at start).
pub type NewSession = Session;

/// The end of a session, decided by the caller (`stats::recorder`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionClose {
    /// Session.
    pub id: SessionId,
    /// End (wall clock).
    pub ended_at: Timestamp,
    /// Monotonic duration.
    pub duration_ms: u64,
    /// Why it ended.
    pub end_reason: EndReason,
    /// Raw exit status.
    pub exit_status: Option<i32>,
    /// Whether it counts towards statistics.
    pub counted: bool,
    /// The duration split into local days (`stats::rollup::split`); used only when counted.
    pub pieces: Vec<PlaytimePiece>,
}

/// One local-day piece of a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlaytimePiece {
    /// Local date `YYYYMMDD`.
    pub day: u32,
    /// Milliseconds on that day.
    pub ms: u64,
    /// Whether the session started on that day.
    pub starts_here: bool,
}

/// Result of a guarded close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseOutcome {
    /// This call closed the row and applied the aggregates.
    Closed {
        /// The game whose aggregates changed.
        game_id: Option<GameId>,
        /// Whether playtime was added.
        counted: bool,
    },
    /// The row was already closed by someone else; nothing changed.
    AlreadyClosed,
    /// No such session.
    NotFound,
}

/// A `session_events` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEvent {
    /// Row key (assigned by the database; ignored on insert).
    pub id: i64,
    /// Session.
    pub session_id: SessionId,
    /// When.
    pub at: Timestamp,
    /// `phase`, `state`, `stop_requested`, `error`, `note`.
    pub kind: String,
    /// JSON payload.
    pub data_json: String,
}

/// Total playtime of one day across games.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DailyPlaytime {
    /// Local date `YYYYMMDD`.
    pub day: u32,
    /// Milliseconds.
    pub ms: u64,
    /// Sessions started that day.
    pub sessions: u32,
}

/// A `slr_installs` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlrInstall {
    /// Variant.
    pub variant: SlrVariant,
    /// Installed version.
    pub version: String,
    /// Directory.
    pub path: PathBuf,
    /// Platform directory name.
    pub platform_dir: PathBuf,
    /// Tree hash of the integrity manifest.
    pub manifest_tree_sha256: Option<Sha256>,
    /// When the manifest was built.
    pub manifest_built_at: Option<Timestamp>,
    /// Size on disk.
    pub size_bytes: Option<u64>,
}

/// A `runtime_audit` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditState {
    /// Variant.
    pub variant: SlrVariant,
    /// An audit is pending (set by supervisors after untrusted sessions).
    pub pending: bool,
    /// Last audit run.
    pub last_run_at: Option<Timestamp>,
    /// Last clean result.
    pub last_clean_at: Option<Timestamp>,
    /// `clean`, `tampered` or `missing`.
    pub last_result: Option<String>,
}

/// A `runtime_events` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEvent {
    /// Row key (assigned by the database; ignored on insert).
    pub id: i64,
    /// When.
    pub at: Timestamp,
    /// Variant, if the event concerns one.
    pub variant: Option<SlrVariant>,
    /// `provisioned`, `updated`, `audit_clean`, `audit_tampered`, `quarantined`, `removed`.
    pub kind: String,
    /// JSON summary.
    pub summary_json: String,
}

/// Keys of the `meta` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetaKey {
    /// Library creation time (milliseconds).
    CreatedAt,
    /// Random 128-bit hex identifier (PERSISTENCE §11).
    InstallId,
    /// Version of the EasyPlay that last opened the library.
    LastAppVersion,
    /// Time of the last daily backup (milliseconds).
    LastBackupAt,
}

impl MetaKey {
    /// Column text.
    pub const fn as_str(self) -> &'static str {
        match self {
            MetaKey::CreatedAt => "created_at",
            MetaKey::InstallId => "install_id",
            MetaKey::LastAppVersion => "last_app_version",
            MetaKey::LastBackupAt => "last_backup_at",
        }
    }
}

// ---- Traits ------------------------------------------------------------------------------

/// Games (`games`) and the library snapshot.
pub trait GameRepo {
    /// Insert a validated game with its executables in one transaction.
    fn insert_game(&mut self, game: &Game) -> StorageResult<()>;
    /// A game by ID, including soft-deleted ones.
    fn game_record(&mut self, id: GameId) -> StorageResult<Option<GameRecord>>;
    /// A game by ID as a domain value (`None` also for soft-deleted games).
    fn game(&mut self, id: GameId) -> StorageResult<Option<Game>>;
    /// Apply a patch; fails with `NotFound` for unknown or deleted games.
    fn update_game(&mut self, id: GameId, patch: &GamePatch, now: Timestamp) -> StorageResult<()>;
    /// Set or clear `deleted_at` (LOGIC_SPECS §11 "Detach" and its undo on failure).
    fn set_game_deleted(&mut self, id: GameId, at: Option<Timestamp>) -> StorageResult<bool>;
    /// Games with `deleted_at` set (start-up purge retry).
    fn deleted_games(&mut self) -> StorageResult<Vec<GameId>>;
    /// Delete the game row and everything that cascades (executables, tag links, artwork,
    /// `game_stats`). With `keep_history`, sessions keep their rows (`game_id` becomes
    /// `NULL`) and `playtime_daily` moves to `removed:<name>`; otherwise both are deleted.
    fn purge_game(&mut self, id: GameId, keep_history: bool) -> StorageResult<bool>;
    /// The five scans of DATA_MODEL §6.2 in one snapshot.
    fn load_library(&mut self) -> StorageResult<LibraryRows>;
    /// Supervisor single-column update (DATA_MODEL §6.1).
    fn set_prefix_tainted_at(&mut self, id: GameId, at: Option<Timestamp>) -> StorageResult<bool>;
    /// Supervisor single-column update (DATA_MODEL §6.1).
    fn set_last_launched_at(&mut self, id: GameId, at: Timestamp) -> StorageResult<bool>;
    /// Number of games that are not soft-deleted.
    fn game_count(&mut self) -> StorageResult<u64>;
}

/// Executables of games (`executables`).
pub trait ExecutableRepo {
    /// Add an executable to a game (at most 32 per game; one primary).
    fn add_executable(&mut self, game: GameId, exe: &Executable) -> StorageResult<()>;
    /// Replace every column of an executable.
    fn update_executable(&mut self, exe: &Executable) -> StorageResult<()>;
    /// Remove an executable; the primary cannot be removed.
    fn remove_executable(&mut self, id: ExecutableId) -> StorageResult<bool>;
    /// An executable and its game.
    fn executable(&mut self, id: ExecutableId) -> StorageResult<Option<(GameId, Executable)>>;
    /// Executables of a game, primary first.
    fn executables_of(&mut self, game: GameId) -> StorageResult<Vec<Executable>>;
    /// Lookup by canonical path (unique index).
    fn executable_by_canonical_path(
        &mut self,
        path: &Path,
    ) -> StorageResult<Option<(GameId, Executable)>>;
    /// Lookup by quick hash (`executables_quick_hash`), excluding soft-deleted games.
    fn executables_by_quick_hash(
        &mut self,
        hash: &Sha256,
    ) -> StorageResult<Vec<(GameId, Executable)>>;
    /// Make `exe` the primary of `game`; the old primary becomes an alternate.
    fn set_primary_executable(&mut self, game: GameId, exe: ExecutableId) -> StorageResult<()>;
}

/// Tags (`tags`, `game_tags`). Limits of GAME_LIBRARY §13.
pub trait TagRepo {
    /// Create a tag (unique case-insensitively, at most 200).
    fn create_tag(&mut self, name: &TagName, color: TagColor, now: Timestamp)
    -> StorageResult<Tag>;
    /// A tag by name, compared case-insensitively.
    fn tag_by_name(&mut self, name: &TagName) -> StorageResult<Option<Tag>>;
    /// All tags with the number of games (not soft-deleted) carrying each, by name.
    fn tags_with_counts(&mut self) -> StorageResult<Vec<(Tag, u32)>>;
    /// Rename; fails with `DuplicateTag` when another tag has the name (merge instead).
    fn rename_tag(&mut self, id: TagId, name: &TagName) -> StorageResult<()>;
    /// Change the colour.
    fn set_tag_color(&mut self, id: TagId, color: TagColor) -> StorageResult<()>;
    /// Move every link of `from` to `into` and delete `from`; returns the games affected.
    fn merge_tags(&mut self, from: TagId, into: TagId) -> StorageResult<u32>;
    /// Delete a tag; returns the number of games it was removed from.
    fn delete_tag(&mut self, id: TagId) -> StorageResult<u32>;
    /// Tags of a game.
    fn tags_of(&mut self, game: GameId) -> StorageResult<Vec<TagId>>;
    /// Replace a game's tags atomically (at most 16).
    fn set_game_tags(&mut self, game: GameId, tags: &[TagId]) -> StorageResult<()>;
    /// Add a tag to several games (bulk action); games already at 16 tags are skipped and
    /// returned.
    fn add_tag_to_games(&mut self, tag: TagId, games: &[GameId]) -> StorageResult<Vec<GameId>>;
}

/// Artwork rows (`artwork`).
pub trait ArtworkRepo {
    /// Insert or replace the picture of a slot.
    fn upsert_artwork(&mut self, art: &ArtworkRef) -> StorageResult<()>;
    /// Pictures of a game.
    fn artwork_of(&mut self, game: GameId) -> StorageResult<Vec<ArtworkRef>>;
    /// Remove a slot's picture.
    fn delete_artwork(&mut self, game: GameId, slot: ArtSlot) -> StorageResult<bool>;
    /// Every hash referenced by a row.
    fn referenced_artwork_hashes(&mut self) -> StorageResult<BTreeSet<Sha256>>;
    /// The subset of `candidates` (hashes found on disk) that no row references.
    fn unreferenced_artwork(&mut self, candidates: &[Sha256]) -> StorageResult<Vec<Sha256>>;
}

/// Sessions (`sessions`, `session_events`).
pub trait SessionRepo {
    /// Insert a new session row.
    fn insert_session(&mut self, session: &Session) -> StorageResult<()>;
    /// A session by ID.
    fn session(&mut self, id: SessionId) -> StorageResult<Option<Session>>;
    /// Change the state of an open session; `false` if it is ended or unknown.
    fn set_session_state(&mut self, id: SessionId, state: SessionState) -> StorageResult<bool>;
    /// Heartbeat: `active_ms` and `last_heartbeat_at` of an open session.
    fn heartbeat(&mut self, id: SessionId, active_ms: u64, at: Timestamp) -> StorageResult<bool>;
    /// Record the sandbox process of an open session.
    fn set_session_sandbox(
        &mut self,
        id: SessionId,
        sandbox_pid: Option<u32>,
        pid_ns: Option<u64>,
    ) -> StorageResult<bool>;
    /// The guarded close of LOGIC_SPECS §7: one transaction that ends the row
    /// `WHERE state != 'ended'` and, only if this call ended it, applies `game_stats`,
    /// `playtime_daily` and `games.last_launched_at`.
    fn close_session(&mut self, close: &SessionClose) -> StorageResult<CloseOutcome>;
    /// Sessions that are not ended (`sessions_open`).
    fn open_sessions(&mut self) -> StorageResult<Vec<Session>>;
    /// History newest first, paged; `game = None` for all games.
    fn session_history(
        &mut self,
        game: Option<GameId>,
        kinds: &[SessionKind],
        offset: u64,
        limit: u64,
    ) -> StorageResult<Vec<Session>>;
    /// Delete a session; if it was counted, subtract it (with its `pieces`) from the
    /// aggregates and recompute `longest_ms`, `first_played_at`, `last_played_at`.
    fn delete_session(&mut self, id: SessionId, pieces: &[PlaytimePiece]) -> StorageResult<bool>;
    /// Append an event.
    fn add_session_event(
        &mut self,
        session: SessionId,
        at: Timestamp,
        kind: &str,
        data_json: &str,
    ) -> StorageResult<i64>;
    /// Events of a session in order.
    fn session_events(&mut self, session: SessionId) -> StorageResult<Vec<SessionEvent>>;
    /// Delete events older than `before` (retention, PERSISTENCE §10).
    fn prune_session_events(&mut self, before: Timestamp) -> StorageResult<u64>;
}

/// Aggregates (`game_stats`, `playtime_daily`).
pub trait StatsRepo {
    /// The aggregates of one game.
    fn game_stats(&mut self, game: GameId) -> StorageResult<Option<GameStats>>;
    /// Every `game_stats` row.
    fn all_game_stats(&mut self) -> StorageResult<Vec<GameStats>>;
    /// Replace a game's aggregates (`stats::rebuild`).
    fn put_game_stats(&mut self, stats: &GameStats) -> StorageResult<()>;
    /// Add to a `playtime_daily` row.
    fn add_playtime(
        &mut self,
        key: &GameKey,
        day: u32,
        ms: u64,
        sessions: u32,
    ) -> StorageResult<()>;
    /// Rows with `from <= day <= to`, optionally for one key.
    fn playtime_range(
        &mut self,
        key: Option<&GameKey>,
        from: u32,
        to: u32,
    ) -> StorageResult<Vec<PlaytimeDay>>;
    /// Per-day totals across all keys in a range.
    fn daily_totals(&mut self, from: u32, to: u32) -> StorageResult<Vec<DailyPlaytime>>;
    /// Sum of milliseconds in a range, optionally for one key.
    fn playtime_sum(&mut self, key: Option<&GameKey>, from: u32, to: u32) -> StorageResult<u64>;
    /// Milliseconds per key in a range (one grouped scan of the `day` index), keys with a
    /// positive sum only, ordered by key text. Includes kept history (`removed:<name>`).
    fn playtime_by_key(&mut self, from: u32, to: u32) -> StorageResult<Vec<(GameKey, u64)>>;
    /// Days in a range on which anything was played (`Σ ms > 0`), ascending.
    fn played_days(&mut self, from: u32, to: u32) -> StorageResult<Vec<u32>>;
    /// Delete every aggregate row (before `stats::rebuild`).
    fn clear_aggregates(&mut self) -> StorageResult<()>;
}

/// The runtime catalogue (`runtimes`, `slr_installs`, `runtime_audit`, `runtime_events`).
pub trait RuntimeRepo {
    /// Replace the whole catalogue in one transaction.
    fn replace_runtime_catalog(&mut self, builds: &[RuntimeBuild]) -> StorageResult<()>;
    /// Insert or replace `upsert` and delete `remove`, in one transaction (incremental
    /// catalogue synchronisation, `runtime::catalog::CatalogDiff`).
    fn apply_runtime_changes(
        &mut self,
        upsert: &[RuntimeBuild],
        remove: &[RuntimeName],
    ) -> StorageResult<()>;
    /// The catalogue ordered by family and version key, newest first.
    fn runtimes(&mut self) -> StorageResult<Vec<RuntimeBuild>>;
    /// One build.
    fn runtime(&mut self, name: &RuntimeName) -> StorageResult<Option<RuntimeBuild>>;
    /// Record use by a launch.
    fn set_runtime_last_used(&mut self, name: &RuntimeName, at: Timestamp) -> StorageResult<bool>;
    /// Insert or replace an SLR install.
    fn put_slr_install(&mut self, install: &SlrInstall) -> StorageResult<()>;
    /// All SLR installs.
    fn slr_installs(&mut self) -> StorageResult<Vec<SlrInstall>>;
    /// Remove an SLR install.
    fn delete_slr_install(&mut self, variant: SlrVariant) -> StorageResult<bool>;
    /// Audit state of a variant.
    fn audit_state(&mut self, variant: SlrVariant) -> StorageResult<Option<AuditState>>;
    /// Mark an audit pending (supervisors).
    fn set_audit_pending(&mut self, variant: SlrVariant) -> StorageResult<()>;
    /// Store an audit result; clears `pending`, sets `last_clean_at` when `clean`.
    fn record_audit_result(
        &mut self,
        variant: SlrVariant,
        at: Timestamp,
        result: &str,
    ) -> StorageResult<()>;
    /// Append a runtime event.
    fn add_runtime_event(&mut self, event: &RuntimeEvent) -> StorageResult<i64>;
    /// Newest events first.
    fn runtime_events(&mut self, limit: u64) -> StorageResult<Vec<RuntimeEvent>>;
}

/// UI state (`kv`) and library metadata (`meta`).
pub trait KvRepo {
    /// A `kv` value.
    fn kv_get(&mut self, key: &str) -> StorageResult<Option<String>>;
    /// Set a `kv` value.
    fn kv_set(&mut self, key: &str, value: &str, now: Timestamp) -> StorageResult<()>;
    /// Delete a `kv` value.
    fn kv_delete(&mut self, key: &str) -> StorageResult<bool>;
    /// Every `kv` pair whose key starts with `prefix`, by key.
    fn kv_with_prefix(&mut self, prefix: &str) -> StorageResult<Vec<(String, String)>>;
    /// A `meta` value.
    fn meta_get(&mut self, key: MetaKey) -> StorageResult<Option<String>>;
    /// Set a `meta` value.
    fn meta_set(&mut self, key: MetaKey, value: &str) -> StorageResult<()>;
}

// ---- Conversion helpers shared by the repository files -----------------------------------

pub(crate) fn path_err(e: PathCodecError) -> StorageError {
    StorageError::Decode(format!("path: {e}"))
}

pub(crate) fn enc_path(p: &Path) -> StorageResult<String> {
    path_codec::encode(p).map_err(|_| {
        StorageError::Invalid(crate::model::ValidationError::new(
            "path",
            crate::model::Problem::NotAbsolute,
        ))
    })
}

pub(crate) fn enc_path_opt(p: Option<&Path>) -> StorageResult<Option<String>> {
    p.map(enc_path).transpose()
}

pub(crate) fn dec_path(s: &str) -> StorageResult<PathBuf> {
    path_codec::decode(s).map_err(path_err)
}

pub(crate) fn dec_path_opt(s: Option<&str>) -> StorageResult<Option<PathBuf>> {
    s.map(dec_path).transpose()
}

pub(crate) fn ts(ms: i64) -> Timestamp {
    Timestamp::from_millis(ms)
}

pub(crate) fn ts_opt(ms: Option<i64>) -> Option<Timestamp> {
    ms.map(Timestamp::from_millis)
}

pub(crate) fn ms_opt(t: Option<Timestamp>) -> Option<i64> {
    t.map(Timestamp::as_millis)
}

/// `u64` → `INTEGER`, saturating at `i64::MAX`.
pub(crate) fn int(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// `INTEGER` → `u64`, negative values (never written) read as 0.
pub(crate) fn uint(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

pub(crate) fn uint32(v: i64) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

/// Text form of a unit-variant serde enum (`snake_case`).
pub(crate) fn enum_text<T: Serialize>(v: &T) -> StorageResult<String> {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        Ok(other) => Ok(other.to_string()),
        Err(e) => Err(StorageError::Decode(e.to_string())),
    }
}

/// Parse [`enum_text`].
pub(crate) fn enum_parse<T: DeserializeOwned>(what: &str, s: &str) -> StorageResult<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|_| StorageError::Decode(format!("{what}: unknown value {s:?}")))
}

pub(crate) fn json_text<T: Serialize>(v: &T) -> StorageResult<String> {
    serde_json::to_string(v).map_err(|e| StorageError::Decode(e.to_string()))
}

pub(crate) fn json_parse<T: DeserializeOwned>(what: &str, s: &str) -> StorageResult<T> {
    serde_json::from_str(s).map_err(|e| StorageError::Decode(format!("{what}: {e}")))
}

pub(crate) fn id_parse<T: std::str::FromStr>(what: &str, s: &str) -> StorageResult<T> {
    s.parse()
        .map_err(|_| StorageError::Decode(format!("{what}: invalid identifier {s:?}")))
}

pub(crate) fn runtime_name(s: &str) -> StorageResult<RuntimeName> {
    RuntimeName::new(s).map_err(|e| StorageError::Decode(e.to_string()))
}

pub(crate) fn hash32(what: &str, v: &[u8]) -> StorageResult<Sha256> {
    <Sha256>::try_from(v).map_err(|_| StorageError::Decode(format!("{what}: not 32 bytes")))
}

pub(crate) fn not_found(what: &'static str, key: impl std::fmt::Display) -> StorageError {
    StorageError::NotFound {
        what,
        key: key.to_string(),
    }
}

/// `TagName` from a stored value.
pub(crate) fn tag_name(s: &str) -> StorageResult<TagName> {
    TagName::new(s).map_err(|e| StorageError::Decode(e.to_string()))
}

pub(crate) fn tag_color(v: i64) -> StorageResult<TagColor> {
    u8::try_from(v)
        .ok()
        .and_then(|c| TagColor::new(c).ok())
        .ok_or_else(|| StorageError::Decode(format!("tag color {v}")))
}

pub(crate) fn fingerprint_parts(fp: &Fingerprint) -> (i64, i64, Vec<u8>, Option<Vec<u8>>) {
    (
        int(fp.size),
        fp.mtime_ns,
        fp.quick_hash.to_vec(),
        fp.full_hash.map(|h| h.to_vec()),
    )
}

/// A bound statement argument.
#[derive(Clone, Debug)]
pub(crate) enum Arg {
    Int(Option<i64>),
    Text(Option<String>),
    Blob(Option<Vec<u8>>),
}

impl Arg {
    pub(crate) fn int(v: i64) -> Self {
        Arg::Int(Some(v))
    }
    pub(crate) fn text(v: impl Into<String>) -> Self {
        Arg::Text(Some(v.into()))
    }
}

/// Execute `sql` with positional `args`; returns the number of changed rows.
pub(crate) async fn exec_args(
    c: &mut super::db::SqlConn,
    sql: &str,
    args: &[Arg],
) -> StorageResult<u64> {
    let mut q = ormlite::query(ormlite::AssertSqlSafe(sql.to_owned()));
    for a in args {
        q = match a {
            Arg::Int(v) => q.bind(*v),
            Arg::Text(v) => q.bind(v.clone()),
            Arg::Blob(v) => q.bind(v.clone()),
        };
    }
    q.execute(&mut *c)
        .await
        .map(|r| r.rows_affected())
        .map_err(super::db::map_sqlx)
}

/// Run a closure on the store's connection (shorthand used by every repository file).
pub(crate) fn conn<S: Store + ?Sized>(s: &mut S) -> &mut super::db::Conn {
    super::db::Sealed::conn(s)
}
