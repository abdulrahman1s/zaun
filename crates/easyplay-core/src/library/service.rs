//! The library service (LIB-004, LIB-005, LIB-012): create from a draft (transaction,
//! directories, compensation), validated updates with the cross-field rules of
//! CONFIGURATION §3, executables, tags, favourites and hidden games with undo data, the
//! start-up orphan sweep, and the snapshot it keeps current.
//!
//! [`Library`] owns the read-write [`Db`] handle of the AppService thread; the AppService
//! reaches other repositories through [`Library::db_mut`]. Relinking ([`super::relink`]),
//! removal ([`super::remove`]), prefix operations ([`super::prefix_ops`]) and installer
//! conversion ([`super::installer`]) add their own `impl Library` blocks.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::draft::GameDraft;
use super::index::{self, ExeIndex, GameRows, LibrarySnapshot, Overlays, RuntimeInfo};
use super::relink::RelinkOutcome;
use super::remove::{RemoveOptions, RemoveReport};
use super::{invalid, limit, no_game};
use crate::artwork::ImageCodec;
use crate::artwork::icon_extract::ICON_LIMITS;
use crate::artwork::import::{normalise, store_and_commit};
use crate::clock::{Clock, Timestamp};
use crate::consts::MAX_EXECUTABLES_PER_GAME;
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{FileType, Fs, FsErrorKind, MountTable, safe_remove_tree};
use crate::host::HostFacts;
use crate::identity::fingerprint;
use crate::identity::pe::{PeInspector, PeLimits};
use crate::identity::{SharedLocations, naming};
use crate::ids::{ExecutableId, GameId, TagId, TagName};
use crate::model::{
    Activity, ArtOrigin, ArtSlot, Availability, EnvVar, ExeRole, Executable, Game, GameDirAccess,
    PeInfo, Problem, SaveDataLocation, SessionKind, SocketAccess, TagColor, UserDir, WindowSystem,
    WorkingDir,
};
use crate::paths::{AppPaths, normalize};
use crate::sandbox::validate::{ValidationInputs, any_blocking, validate_policy};
use crate::sandbox::{PolicyIssue, SandboxError};
use crate::storage::{
    ArtworkRepo, Db, ExecutableRepo, GamePatch, GameRepo, StatsRepo, StorageError, TagRepo,
};

/// Most tags in the library (GAME_LIBRARY §13).
pub const MAX_TAGS_TOTAL: usize = 200;
/// Most tags per game.
pub const MAX_TAGS_PER_GAME: usize = 16;
/// Orphan directories younger than this are left alone (a create may be in progress).
pub const ORPHAN_MIN_AGE: Duration = Duration::from_secs(3600);
/// Temporary files (`*.tmp-*`) younger than this are left alone.
pub const TEMP_MIN_AGE: Duration = Duration::from_secs(3600);

// ---- Seams ---------------------------------------------------------------------------------

/// Host-dependent policy validation (`sandbox::validate_policy` needs `HostFacts`, the
/// mount table and the user's directories). [`HostPolicyCheck`] is the real implementation;
/// [`NoPolicyCheck`] reports nothing (tests, headless tools).
pub trait PolicyCheck: Send + Sync {
    /// Issues of `game`'s policy; `other_roots` are the folders of the other games.
    fn issues(&self, game: &Game, other_roots: &[PathBuf]) -> Vec<PolicyIssue>;
}

/// Reports no issues.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoPolicyCheck;

impl PolicyCheck for NoPolicyCheck {
    fn issues(&self, _game: &Game, _other_roots: &[PathBuf]) -> Vec<PolicyIssue> {
        Vec::new()
    }
}

/// [`PolicyCheck`] over `sandbox::validate_policy` for `Game` sessions.
pub struct HostPolicyCheck {
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Mount table.
    pub mounts: MountTable,
    /// EasyPlay's locations.
    pub paths: AppPaths,
    /// Host facts.
    pub host: HostFacts,
    /// Real home.
    pub home: PathBuf,
    /// User ID.
    pub uid: u32,
    /// User name.
    pub user_name: String,
    /// XDG user directories.
    pub user_dirs: Vec<(UserDir, PathBuf)>,
    /// Developer Mode.
    pub developer_mode: bool,
    /// Global user environment variables.
    pub global_env: Vec<EnvVar>,
}

impl PolicyCheck for HostPolicyCheck {
    fn issues(&self, game: &Game, other_roots: &[PathBuf]) -> Vec<PolicyIssue> {
        let exe = game.primary().map(|e| e.canonical_path.clone());
        let home = self.paths.home_dir(game.id);
        let prefix = self.paths.prefix_dir(game.id);
        let inputs = ValidationInputs {
            kind: SessionKind::Game,
            fs: &*self.fs,
            mounts: &self.mounts,
            home: &self.home,
            uid: self.uid,
            user_name: &self.user_name,
            paths: &self.paths,
            developer_mode: self.developer_mode,
            game_root: game.root.as_deref(),
            exe: exe.as_deref(),
            private_home: Some(&home),
            prefix: Some(&prefix),
            user_dirs: &self.user_dirs,
            other_game_roots: other_roots,
            compat: Some(&game.launch.compat),
            working_dir: Some(&game.launch.working_dir),
            user_env: [&self.global_env, &game.launch.env],
            host: &self.host,
        };
        validate_policy(&game.policy, &inputs)
    }
}

/// Everything the service needs besides the database.
pub struct LibraryDeps {
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Clock.
    pub clock: Arc<dyn Clock>,
    /// Locations.
    pub paths: AppPaths,
    /// Compatibility-layer labels and order.
    pub runtime: Arc<dyn RuntimeInfo>,
    /// Policy validation.
    pub check: Arc<dyn PolicyCheck>,
    /// PE reader (added executables, relink, installer conversion).
    pub pe: Arc<dyn PeInspector>,
    /// Image codec for draft icons; `None` = icons are not stored.
    pub codec: Option<Arc<dyn ImageCodec>>,
    /// Mount table (stored mount points of added executables).
    pub mounts: MountTable,
    /// Shared locations (game-root rules when re-linking).
    pub shared: SharedLocations,
    /// Overlay support on this host (CONFIGURATION §3 rule 4).
    pub overlay_supported: bool,
}

/// What [`Library::create`] did besides adding the game.
#[derive(Debug)]
pub struct CreateReport {
    /// The new game.
    pub id: GameId,
    /// Its primary executable.
    pub exe: ExecutableId,
    /// Storing the icon failed; the game keeps the generated placeholder (toast).
    pub artwork_error: Option<Error>,
}

/// Non-blocking findings of an update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchWarning {
    /// CONFIGURATION §3 rule 8: going back to an older compatibility layer.
    RuntimeDowngrade,
    /// Non-blocking policy issues.
    Policy(Vec<String>),
}

/// The previous values of a bulk flag change, for the toast's *Undo*.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlagUndo {
    /// `(game, previous value)`.
    pub previous: Vec<(GameId, bool)>,
}

/// The previous tags of a game, for *Undo*.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagsUndo {
    /// The game.
    pub game: Option<GameId>,
    /// Its previous tags.
    pub previous: Vec<TagId>,
}

/// A `games/<id>/` directory without a row (PERSISTENCE §8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrphanDir {
    /// The directory.
    pub path: PathBuf,
    /// The ID its name encodes, when it is one.
    pub id: Option<GameId>,
    /// Last modification.
    pub modified_ns: i64,
    /// Whether it holds anything beyond the empty `home`/`prefix` skeleton.
    pub has_content: bool,
}

/// What the start-up sweep did.
#[derive(Debug, Default)]
pub struct SweepReport {
    /// Empty orphan directories removed.
    pub removed: Vec<PathBuf>,
    /// Orphans with content, left for the user (Settings → Advanced).
    pub listed: Vec<OrphanDir>,
    /// Temporary files removed.
    pub temp_files_removed: usize,
    /// Failures.
    pub errors: Vec<Error>,
}

/// The service interface of GAME_LIBRARY §14 (richer return types: undo data, reports).
pub trait LibraryService {
    /// The current snapshot.
    fn snapshot(&self) -> Arc<LibrarySnapshot>;
    /// Add a game from a confirmed draft.
    fn create(&mut self, draft: &GameDraft) -> Result<CreateReport>;
    /// Apply a validated patch.
    fn update(&mut self, id: GameId, patch: GamePatch) -> Result<Vec<PatchWarning>>;
    /// Replace a game's tags (creating unknown tags).
    fn set_tags(&mut self, id: GameId, tags: &[TagName]) -> Result<TagsUndo>;
    /// Add an executable.
    fn add_executable(&mut self, id: GameId, path: &Path, role: ExeRole) -> Result<ExecutableId>;
    /// Re-link an executable (LOGIC_SPECS §8).
    fn relink(
        &mut self,
        exe: ExecutableId,
        new_path: &Path,
        confirmed: bool,
    ) -> Result<RelinkOutcome>;
    /// Remove a game (LOGIC_SPECS §11).
    fn remove(&mut self, id: GameId, options: RemoveOptions) -> Result<RemoveReport>;
    /// Activity overlay (no database write).
    fn set_activity(&mut self, id: GameId, activity: Activity);
    /// Availability overlay (no database write).
    fn set_availability(&mut self, updates: &[(GameId, Availability)]);
}

/// The library on the AppService thread.
pub struct Library {
    pub(super) db: Db,
    pub(super) deps: LibraryDeps,
    snapshot: Arc<LibrarySnapshot>,
    overlays: Overlays,
    pub(super) exes: ExeIndex,
    flows: HashSet<GameId>,
}

pub(super) fn storage(e: StorageError) -> Error {
    Error::from(e)
}

impl Library {
    /// Load the library and build the first snapshot.
    pub fn open(db: Db, deps: LibraryDeps) -> Result<Self> {
        let now = deps.clock.now();
        let mut lib = Self {
            db,
            deps,
            snapshot: Arc::new(LibrarySnapshot::empty(now)),
            overlays: Overlays::default(),
            exes: ExeIndex::default(),
            flows: HashSet::new(),
        };
        lib.reload()?;
        Ok(lib)
    }

    /// The database handle (other repositories, transactions spanning modules).
    pub fn db_mut(&mut self) -> &mut Db {
        &mut self.db
    }

    /// The dependencies.
    pub fn deps(&self) -> &LibraryDeps {
        &self.deps
    }

    /// The matcher index (`identity::matcher::resolve`).
    pub fn exe_index(&self) -> &ExeIndex {
        &self.exes
    }

    /// The overlays.
    pub fn overlays(&self) -> &Overlays {
        &self.overlays
    }

    /// Give the database back.
    pub fn into_db(self) -> Db {
        self.db
    }

    fn now(&self) -> Timestamp {
        self.deps.clock.now()
    }

    fn publish(&mut self, s: LibrarySnapshot) {
        self.snapshot = Arc::new(s);
    }

    /// Full rebuild from the database (start-up, after bulk operations).
    pub fn reload(&mut self) -> Result<()> {
        let rows = self.db.load_library().map_err(storage)?;
        let rev = self.snapshot.revision + 1;
        let s = index::build(&rows, &*self.deps.runtime, &self.overlays, rev, self.now());
        self.exes = ExeIndex::from_rows(&rows);
        self.publish(s);
        Ok(())
    }

    /// Incremental update of one game from the database.
    pub fn refresh_game(&mut self, id: GameId) -> Result<()> {
        let now = self.now();
        let rec = self.db.game_record(id).map_err(storage)?;
        let Some(rec) = rec.filter(|r| r.deleted_at.is_none()) else {
            self.exes.remove_game(id);
            let s = self.snapshot.without_game(id, now);
            self.publish(s);
            return Ok(());
        };
        let tags = self.db.tags_of(id).map_err(storage)?;
        let art = self.db.artwork_of(id).map_err(storage)?;
        let stats = self.db.game_stats(id).map_err(storage)?;
        let summary = index::summarize(
            GameRows {
                record: &rec,
                tags: &tags,
                artwork: &art,
                stats: stats.as_ref(),
            },
            &*self.deps.runtime,
            &self.overlays,
            self.snapshot.get(id),
        );
        self.exes
            .upsert_game(&rec, stats.as_ref().and_then(|s| s.last_played_at));
        let s = self.snapshot.with_game(summary, now);
        self.publish(s);
        Ok(())
    }

    /// Re-read the tag list.
    pub fn refresh_tags(&mut self) -> Result<()> {
        let tags: Vec<_> = self
            .db
            .tags_with_counts()
            .map_err(storage)?
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        let s = self.snapshot.with_tags(index::tag_rows(&tags), self.now());
        self.publish(s);
        Ok(())
    }

    // ---- flow locks (LOGIC_SPECS preamble) ----------------------------------------------

    /// Take the per-game flow lock; `EP-LIB-020` when another flow holds it.
    pub fn begin_flow(&mut self, id: GameId) -> Result<()> {
        if self.flows.insert(id) {
            Ok(())
        } else {
            Err(Error::new(ErrorCode::LibBusy).with("game", id))
        }
    }

    /// Release the per-game flow lock.
    pub fn end_flow(&mut self, id: GameId) {
        self.flows.remove(&id);
    }

    /// Whether a flow holds the game.
    pub fn in_flow(&self, id: GameId) -> bool {
        self.flows.contains(&id)
    }

    // ---- reading -----------------------------------------------------------------------

    /// A game as a domain value; `EP-INT-001` when unknown or removed.
    pub fn game(&mut self, id: GameId) -> Result<Game> {
        self.db
            .game(id)
            .map_err(storage)?
            .ok_or_else(|| no_game(id))
    }

    fn other_roots(&self, id: Option<GameId>) -> Vec<PathBuf> {
        self.exes.other_roots(id)
    }

    // ---- validation ----------------------------------------------------------------------

    /// Field limits, the cross-field rules of CONFIGURATION §3 and blocking policy issues.
    /// Returns the non-blocking findings.
    pub fn validate_game(&self, game: &Game, previous: Option<&Game>) -> Result<Vec<PatchWarning>> {
        game.validate()?;
        cross_field(
            game,
            &self.deps.paths,
            self.deps.overlay_supported,
            &self.exes,
        )?;
        let mut warnings = Vec::new();
        if let Some(prev) = previous
            && prev.runtime != game.runtime
            && let Some(prepared) = &game.prefix_prepared_with
            && self.deps.runtime.is_downgrade(prepared, &game.runtime)
        {
            warnings.push(PatchWarning::RuntimeDowngrade);
        }
        let issues = self
            .deps
            .check
            .issues(game, &self.other_roots(Some(game.id)));
        let blocking: Vec<PolicyIssue> = issues
            .iter()
            .filter(|i| i.is_blocking())
            .filter(|i| !i.ack_flag().is_some_and(|f| game.flags.contains(f)))
            .cloned()
            .collect();
        if any_blocking(&blocking) {
            return Err(Error::from(SandboxError::Plan(blocking)));
        }
        let notes: Vec<String> = issues
            .iter()
            .filter(|i| !i.is_blocking())
            .map(|i| i.code.as_str().to_owned())
            .collect();
        if !notes.is_empty() {
            warnings.push(PatchWarning::Policy(notes));
        }
        Ok(warnings)
    }

    // ---- create (LIB-004) ----------------------------------------------------------------

    /// Add a game from a confirmed draft: one transaction, then the private directories
    /// (failure → the row is purged again: no partial game), then the icon (failure → the
    /// game keeps the placeholder).
    pub fn create(&mut self, draft: &GameDraft) -> Result<CreateReport> {
        let clock = self.deps.clock.clone();
        let now = clock.now();
        if let Some((owner, _)) = self
            .db
            .executable_by_canonical_path(&draft.canonical_path)
            .map_err(storage)?
        {
            return Err(Error::new(ErrorCode::LibAlreadyInLibrary).with("game", owner));
        }
        let id = GameId::generate(&*clock);
        let exe = ExecutableId::generate(&*clock);
        let game = draft.to_game(id, exe, now);
        self.validate_game(&game, None)?;
        if draft.tags.len() > MAX_TAGS_PER_GAME {
            return Err(limit("game.tags", MAX_TAGS_PER_GAME));
        }
        let tags = draft.tags.clone();
        self.db.transaction(|tx| -> Result<()> {
            tx.insert_game(&game).map_err(storage)?;
            let ids = resolve_tags(tx, &tags, now)?;
            tx.set_game_tags(id, &ids).map_err(storage)?;
            Ok(())
        })?;
        if let Err(e) = self.create_dirs(id) {
            self.compensate_create(id);
            return Err(e);
        }
        let artwork_error = match (&draft.icon, self.deps.codec.clone()) {
            (Some(bytes), Some(codec)) => self.store_icon(id, &*codec, bytes, now).err(),
            _ => None,
        };
        self.refresh_game(id)?;
        if !tags.is_empty() {
            self.refresh_tags()?;
        }
        Ok(CreateReport {
            id,
            exe,
            artwork_error,
        })
    }

    fn create_dirs(&self, id: GameId) -> Result<()> {
        let fs = &*self.deps.fs;
        let p = &self.deps.paths;
        fs.create_dir_all(&p.games_dir, 0o700)?;
        for d in [p.game_dir(id), p.home_dir(id), p.prefix_dir(id)] {
            match fs.create_dir(&d, 0o700) {
                Ok(()) => {}
                Err(e) if e.kind == FsErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn compensate_create(&mut self, id: GameId) {
        if let Err(e) = self.db.purge_game(id, false) {
            tracing::error!(error = %e, game = %id, "compensation after failed create failed");
        }
        let p = &self.deps.paths;
        if let Err(e) = safe_remove_tree(&*self.deps.fs, &p.games_dir, &p.game_dir(id)) {
            tracing::warn!(error = %e, "could not remove partial game directory");
        }
    }

    pub(super) fn store_icon(
        &mut self,
        id: GameId,
        codec: &dyn ImageCodec,
        bytes: &[u8],
        now: Timestamp,
    ) -> Result<()> {
        let n = normalise(codec, ArtSlot::Icon, bytes, &ICON_LIMITS)
            .map_err(|e| Error::new(ErrorCode::ArtUnreadable).with("reason", e.to_string()))?;
        store_and_commit(
            &*self.deps.fs,
            &self.deps.paths,
            &mut self.db,
            id,
            &n,
            ArtOrigin::ExecutableIcon,
            now,
        )?;
        Ok(())
    }

    // ---- update ------------------------------------------------------------------------

    /// Validate and apply a patch (LOGIC_SPECS §10 "game").
    pub fn update(&mut self, id: GameId, mut patch: GamePatch) -> Result<Vec<PatchWarning>> {
        let old = self.game(id)?;
        if let Some(name) = &patch.name {
            let trimmed = name.trim().to_owned();
            patch.sort_name = Some(
                patch
                    .sort_name
                    .clone()
                    .unwrap_or_else(|| naming::sort_name(&trimmed)),
            );
            patch.name = Some(trimmed);
        }
        let new = apply_patch(&old, &patch);
        let warnings = self.validate_game(&new, Some(&old))?;
        self.db
            .update_game(id, &patch, self.now())
            .map_err(storage)?;
        self.refresh_game(id)?;
        Ok(warnings)
    }

    // ---- executables ---------------------------------------------------------------------

    /// Add another program to a game (at most 32). `role = Primary` makes it the primary
    /// and the old primary an alternate.
    pub fn add_executable(
        &mut self,
        id: GameId,
        path: &Path,
        role: ExeRole,
    ) -> Result<ExecutableId> {
        let game = self.game(id)?;
        if game.executables.len() >= MAX_EXECUTABLES_PER_GAME {
            return Err(limit("game.executables", MAX_EXECUTABLES_PER_GAME));
        }
        let exe = self.new_executable(game.root.as_deref(), path, ExeRole::Alternate)?;
        let mut exe = exe;
        if role != ExeRole::Primary {
            exe.role = role;
        }
        let exe_id = exe.id;
        self.db.transaction(|tx| -> Result<()> {
            tx.add_executable(id, &exe).map_err(storage)?;
            if role == ExeRole::Primary {
                tx.set_primary_executable(id, exe_id).map_err(storage)?;
            }
            Ok(())
        })?;
        self.refresh_game(id)?;
        Ok(exe_id)
    }

    /// Fingerprint and inspect a file into a new executable row (not yet stored).
    pub(super) fn new_executable(
        &mut self,
        root: Option<&Path>,
        path: &Path,
        role: ExeRole,
    ) -> Result<Executable> {
        let fs = self.deps.fs.clone();
        let canonical = fs.canonicalize(path)?;
        if let Some((owner, _)) = self
            .db
            .executable_by_canonical_path(&canonical)
            .map_err(storage)?
        {
            return Err(Error::new(ErrorCode::LibAlreadyInLibrary).with("game", owner));
        }
        let fp = fingerprint::compute(&*fs, &canonical)?;
        let pe = self
            .deps
            .pe
            .inspect(&canonical, &PeLimits::default())
            .unwrap_or_else(|_| PeInfo::unknown());
        let now = self.now();
        Ok(Executable {
            id: ExecutableId::generate(&*self.deps.clock),
            role,
            path: path.to_path_buf(),
            rel_path: root
                .and_then(|r| canonical.strip_prefix(r).ok())
                .map(Path::to_path_buf),
            mount_point: self
                .deps
                .mounts
                .mount_point_of(&canonical)
                .map(|m| m.mount_point.clone()),
            canonical_path: canonical,
            fingerprint: fp,
            pe,
            args_override: None,
            label: None,
            added_at: now,
        })
    }

    // ---- tags, favourites, hidden (LIB-005) ------------------------------------------

    /// Replace a game's tags, creating unknown names (16 per game, 200 in total).
    pub fn set_tags(&mut self, id: GameId, tags: &[TagName]) -> Result<TagsUndo> {
        if tags.len() > MAX_TAGS_PER_GAME {
            return Err(limit("game.tags", MAX_TAGS_PER_GAME));
        }
        self.game(id)?;
        let now = self.now();
        let previous = self.db.tags_of(id).map_err(storage)?;
        let tags = tags.to_vec();
        self.db.transaction(|tx| -> Result<()> {
            let ids = resolve_tags(tx, &tags, now)?;
            tx.set_game_tags(id, &ids).map_err(storage)
        })?;
        self.refresh_tags()?;
        self.refresh_game(id)?;
        Ok(TagsUndo {
            game: Some(id),
            previous,
        })
    }

    /// Undo [`Library::set_tags`].
    pub fn undo_tags(&mut self, undo: &TagsUndo) -> Result<()> {
        let Some(id) = undo.game else { return Ok(()) };
        self.db.set_game_tags(id, &undo.previous).map_err(storage)?;
        self.refresh_tags()?;
        self.refresh_game(id)
    }

    /// Create a tag (case-insensitively unique, at most 200).
    pub fn create_tag(&mut self, name: &TagName, color: TagColor) -> Result<TagId> {
        let now = self.now();
        let t = self.db.create_tag(name, color, now).map_err(tag_err)?;
        self.refresh_tags()?;
        Ok(t.id)
    }

    /// Rename a tag; renaming to an existing name merges into it (after the UI confirmed).
    pub fn rename_tag(&mut self, id: TagId, name: &TagName) -> Result<TagId> {
        let target = match self.db.tag_by_name(name).map_err(storage)? {
            Some(other) if other.id != id => {
                self.db.merge_tags(id, other.id).map_err(storage)?;
                other.id
            }
            _ => {
                self.db.rename_tag(id, name).map_err(tag_err)?;
                id
            }
        };
        self.reload()?;
        Ok(target)
    }

    /// Change a tag's colour.
    pub fn set_tag_color(&mut self, id: TagId, color: TagColor) -> Result<()> {
        self.db.set_tag_color(id, color).map_err(storage)?;
        self.refresh_tags()
    }

    /// Delete a tag; returns how many games lost it.
    pub fn delete_tag(&mut self, id: TagId) -> Result<u32> {
        let n = self.db.delete_tag(id).map_err(storage)?;
        self.reload()?;
        Ok(n)
    }

    /// Bulk *Add tag*; games already at 16 tags are skipped and returned.
    pub fn add_tag_to_games(&mut self, tag: TagId, games: &[GameId]) -> Result<Vec<GameId>> {
        let skipped = self.db.add_tag_to_games(tag, games).map_err(storage)?;
        self.refresh_tags()?;
        for g in games {
            self.refresh_game(*g)?;
        }
        Ok(skipped)
    }

    /// Bulk *Remove tag*.
    pub fn remove_tag_from_games(&mut self, tag: TagId, games: &[GameId]) -> Result<()> {
        let games = games.to_vec();
        self.db.transaction(|tx| -> Result<()> {
            for g in &games {
                let mut t = tx.tags_of(*g).map_err(storage)?;
                if t.contains(&tag) {
                    t.retain(|x| *x != tag);
                    tx.set_game_tags(*g, &t).map_err(storage)?;
                }
            }
            Ok(())
        })?;
        self.refresh_tags()?;
        for g in games {
            self.refresh_game(g)?;
        }
        Ok(())
    }

    fn set_flag(
        &mut self,
        games: &[GameId],
        value: bool,
        get: fn(&Game) -> bool,
        patch: fn(bool) -> GamePatch,
    ) -> Result<FlagUndo> {
        let now = self.now();
        let mut previous = Vec::new();
        for id in games {
            previous.push((*id, get(&self.game(*id)?)));
        }
        let ids = games.to_vec();
        self.db.transaction(|tx| -> Result<()> {
            for id in &ids {
                tx.update_game(*id, &patch(value), now).map_err(storage)?;
            }
            Ok(())
        })?;
        for id in games {
            self.refresh_game(*id)?;
        }
        Ok(FlagUndo { previous })
    }

    /// Favourite or unfavourite games.
    pub fn set_favorite(&mut self, games: &[GameId], value: bool) -> Result<FlagUndo> {
        self.set_flag(
            games,
            value,
            |g| g.favorite,
            |v| GamePatch {
                favorite: Some(v),
                ..GamePatch::default()
            },
        )
    }

    /// Hide or unhide games.
    pub fn set_hidden(&mut self, games: &[GameId], value: bool) -> Result<FlagUndo> {
        self.set_flag(
            games,
            value,
            |g| g.hidden,
            |v| GamePatch {
                hidden: Some(v),
                ..GamePatch::default()
            },
        )
    }

    /// Undo a favourite change.
    pub fn undo_favorite(&mut self, undo: &FlagUndo) -> Result<()> {
        for (id, v) in &undo.previous {
            self.set_favorite(&[*id], *v)?;
        }
        Ok(())
    }

    /// Undo a hidden change.
    pub fn undo_hidden(&mut self, undo: &FlagUndo) -> Result<()> {
        for (id, v) in &undo.previous {
            self.set_hidden(&[*id], *v)?;
        }
        Ok(())
    }

    // ---- overlays ----------------------------------------------------------------------

    /// Activity overlay (no database write).
    pub fn set_activity(&mut self, id: GameId, activity: Activity) {
        self.overlays.set_activity(id, activity);
        let s = self.snapshot.with_overlays(&self.overlays, self.now());
        self.publish(s);
    }

    /// Availability overlay (no database write).
    pub fn set_availability(&mut self, updates: &[(GameId, Availability)]) {
        self.overlays.set_availability(updates);
        let s = self.snapshot.with_overlays(&self.overlays, self.now());
        self.publish(s);
    }

    /// Dominant-cover-colour overlay (no database write).
    pub fn set_accents(&mut self, updates: &[(GameId, Option<[u8; 3]>)]) {
        if updates.is_empty() {
            return;
        }
        self.overlays.set_accents(updates);
        let s = self.snapshot.with_overlays(&self.overlays, self.now());
        self.publish(s);
    }

    // ---- orphans (LIB-012) -------------------------------------------------------------

    /// `games/<id>/` directories without a row (soft-deleted rows still own theirs).
    pub fn orphans(&mut self) -> Result<Vec<OrphanDir>> {
        let fs = self.deps.fs.clone();
        let games_dir = self.deps.paths.games_dir.clone();
        let entries = match fs.read_dir(&games_dir) {
            Ok(e) => e,
            Err(e) if e.kind == FsErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for entry in entries {
            if entry.file_type != FileType::Dir {
                continue;
            }
            let id: Option<GameId> = entry.name.to_str().and_then(|s| s.parse().ok());
            if let Some(id) = id
                && self.db.game_record(id).map_err(storage)?.is_some()
            {
                continue;
            }
            let md = fs.metadata(&entry.path)?;
            out.push(OrphanDir {
                has_content: has_content(&*fs, &entry.path),
                modified_ns: md.mtime_ns,
                path: entry.path,
                id,
            });
        }
        Ok(out)
    }

    /// Start-up sweep: empty orphan directories (failed creates) older than one hour are
    /// removed; orphans with content are listed for the user (PERSISTENCE §8 offers
    /// re-attach or delete); `*.tmp-*` files older than one hour are removed.
    pub fn sweep_orphans(&mut self) -> Result<SweepReport> {
        let fs = self.deps.fs.clone();
        let now = self.now();
        let cutoff = now.saturating_sub(ORPHAN_MIN_AGE).as_millis() * 1_000_000;
        let mut report = SweepReport::default();
        for o in self.orphans()? {
            if o.modified_ns > cutoff {
                continue;
            }
            if o.has_content {
                report.listed.push(o);
                continue;
            }
            match safe_remove_tree(&*fs, &self.deps.paths.games_dir, &o.path) {
                Ok(r) if r.is_complete() => report.removed.push(o.path),
                Ok(r) => report.errors.extend(r.errors.into_iter().map(Error::from)),
                Err(e) => report.errors.push(e.into()),
            }
        }
        let temp_cutoff = now.saturating_sub(TEMP_MIN_AGE).as_millis() * 1_000_000;
        let mut dirs = vec![
            self.deps.paths.data_dir.clone(),
            self.deps.paths.config_dir.clone(),
        ];
        if let Ok(arts) = fs.read_dir(&self.deps.paths.artwork_dir) {
            dirs.extend(
                arts.into_iter()
                    .filter(|e| e.file_type == FileType::Dir)
                    .map(|e| e.path),
            );
        }
        for dir in dirs {
            let Ok(entries) = fs.read_dir(&dir) else {
                continue;
            };
            for e in entries {
                let is_temp = e.file_type == FileType::File
                    && e.name.to_str().is_some_and(|n| n.contains(".tmp-"));
                if !is_temp {
                    continue;
                }
                let old = fs
                    .metadata(&e.path)
                    .is_ok_and(|m| m.mtime_ns <= temp_cutoff);
                if old {
                    match fs.remove_file(&e.path) {
                        Ok(()) => report.temp_files_removed += 1,
                        Err(err) => report.errors.push(err.into()),
                    }
                }
            }
        }
        Ok(report)
    }

    /// Delete one listed orphan (Settings → Advanced → "Orphaned game data").
    pub fn delete_orphan(&mut self, orphan: &OrphanDir) -> Result<()> {
        if let Some(id) = orphan.id
            && self.db.game_record(id).map_err(storage)?.is_some()
        {
            return Err(Error::new(ErrorCode::LibBusy).with("game", id));
        }
        let r = safe_remove_tree(&*self.deps.fs, &self.deps.paths.games_dir, &orphan.path)?;
        match r.errors.into_iter().next() {
            Some(e) => Err(e.into()),
            None => Ok(()),
        }
    }
}

fn tag_err(e: StorageError) -> Error {
    match e {
        StorageError::Limit { what, max } => limit(what, max),
        StorageError::DuplicateTag => invalid("tag.name", Problem::Duplicate),
        other => storage(other),
    }
}

/// Tag IDs for names, creating the missing ones (colour 0).
fn resolve_tags<S: TagRepo + ?Sized>(
    store: &mut S,
    names: &[TagName],
    now: Timestamp,
) -> Result<Vec<TagId>> {
    let mut ids: Vec<TagId> = Vec::new();
    for n in names {
        let id = match store.tag_by_name(n).map_err(storage)? {
            Some(t) => t.id,
            None => {
                let count = store.tags_with_counts().map_err(storage)?.len();
                if count >= MAX_TAGS_TOTAL {
                    return Err(limit("tags", MAX_TAGS_TOTAL));
                }
                store
                    .create_tag(n, TagColor::default(), now)
                    .map_err(tag_err)?
                    .id
            }
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn has_content(fs: &dyn Fs, dir: &Path) -> bool {
    let Ok(entries) = fs.read_dir(dir) else {
        return true;
    };
    entries.iter().any(|e| {
        let skeleton = e.file_type == FileType::Dir
            && matches!(e.name.to_str(), Some("home" | "prefix"))
            && fs.read_dir(&e.path).is_ok_and(|v| v.is_empty());
        !skeleton
    })
}

/// The game `patch` would produce (pure).
pub fn apply_patch(old: &Game, p: &GamePatch) -> Game {
    let mut g = old.clone();
    if let Some(v) = &p.name {
        g.name = v.clone();
        g.sort_name = p.sort_name.clone().unwrap_or_else(|| naming::sort_name(v));
    } else if let Some(s) = &p.sort_name {
        g.sort_name = s.clone();
    }
    macro_rules! set {
        ($($f:ident),*) => { $( if let Some(v) = &p.$f { g.$f = v.clone(); } )* };
    }
    set!(
        name_is_user_set,
        root,
        favorite,
        hidden,
        notes,
        policy,
        launch,
        umu_id,
        store,
        runtime,
        runtime_follow_updates,
        runtime_last_resolved,
        prefix_prepared_with,
        flags,
        steam_app_id,
        steam_app_id_is_user_set
    );
    g
}

/// CONFIGURATION §3 cross-field rules 1–5 and 7 (rule 6 adds nothing, rule 8 is a warning).
pub fn cross_field(
    game: &Game,
    paths: &AppPaths,
    overlay_supported: bool,
    others: &ExeIndex,
) -> Result<()> {
    // Rule 1 is part of `Game::validate`.
    let prefix = paths.prefix_dir(game.id);
    let home = paths.home_dir(game.id);
    let primary_in_prefix = game
        .primary()
        .is_some_and(|e| normalize(&e.canonical_path).starts_with(&prefix));
    // Rule 2.
    if let WorkingDir::Custom(p) = &game.launch.working_dir {
        let p = normalize(p);
        let inside = game
            .root
            .as_deref()
            .is_some_and(|r| p.starts_with(normalize(r)))
            || game
                .policy
                .folders
                .iter()
                .any(|f| p.starts_with(normalize(&f.canonical)))
            || p.starts_with(&prefix)
            || p.starts_with(&home);
        if !inside {
            return Err(invalid("launch.working_dir", Problem::OutOfRange));
        }
    }
    // Rule 3. A game whose program was installed into its prefix keeps whatever kind the
    // installer had (LOGIC_SPECS §13), so only the two unambiguous cases are enforced.
    let file_only = game.policy.game_dir == GameDirAccess::FileOnly;
    if game.root.is_some() && file_only {
        return Err(invalid("policy.game_dir", Problem::InvalidFormat));
    }
    if game.root.is_none() && !primary_in_prefix && !file_only {
        return Err(invalid("policy.game_dir", Problem::InvalidFormat));
    }
    // Rule 4.
    if game.policy.game_dir == GameDirAccess::Protected && !overlay_supported {
        return Err(invalid("policy.game_dir", Problem::Reserved));
    }
    // Rule 5.
    let d = &game.policy.display;
    if d.wayland == SocketAccess::Deny && d.x11 == SocketAccess::Deny {
        return Err(invalid("policy.display", Problem::OutOfRange));
    }
    match game.launch.compat.window_system {
        WindowSystem::Wayland if d.wayland != SocketAccess::Auto => {
            return Err(invalid("launch.compat.window_system", Problem::OutOfRange));
        }
        WindowSystem::X11 if d.x11 != SocketAccess::Auto => {
            return Err(invalid("launch.compat.window_system", Problem::OutOfRange));
        }
        _ => {}
    }
    // Rule 7.
    if let SaveDataLocation::External { dir } = &game.policy.save_data {
        let dir = normalize(dir);
        if let Some(root) = &game.root
            && dir.starts_with(normalize(root))
            && matches!(
                game.policy.game_dir,
                GameDirAccess::ReadOnly | GameDirAccess::Protected
            )
        {
            return Err(invalid("policy.save_data", Problem::OutOfRange));
        }
        if others
            .other_save_dirs(Some(game.id))
            .iter()
            .map(|o| normalize(o))
            .any(|o| o.starts_with(&dir) || dir.starts_with(&o))
        {
            return Err(invalid("policy.save_data", Problem::Duplicate));
        }
    }
    Ok(())
}

impl LibraryService for Library {
    fn snapshot(&self) -> Arc<LibrarySnapshot> {
        self.snapshot.clone()
    }

    fn create(&mut self, draft: &GameDraft) -> Result<CreateReport> {
        Library::create(self, draft)
    }

    fn update(&mut self, id: GameId, patch: GamePatch) -> Result<Vec<PatchWarning>> {
        Library::update(self, id, patch)
    }

    fn set_tags(&mut self, id: GameId, tags: &[TagName]) -> Result<TagsUndo> {
        Library::set_tags(self, id, tags)
    }

    fn add_executable(&mut self, id: GameId, path: &Path, role: ExeRole) -> Result<ExecutableId> {
        Library::add_executable(self, id, path, role)
    }

    fn relink(
        &mut self,
        exe: ExecutableId,
        new_path: &Path,
        confirmed: bool,
    ) -> Result<RelinkOutcome> {
        Library::relink(self, exe, new_path, confirmed)
    }

    fn remove(&mut self, id: GameId, options: RemoveOptions) -> Result<RemoveReport> {
        Library::remove(self, id, options, &super::remove::NoHooks)
    }

    fn set_activity(&mut self, id: GameId, activity: Activity) {
        Library::set_activity(self, id, activity);
    }

    fn set_availability(&mut self, updates: &[(GameId, Availability)]) {
        Library::set_availability(self, updates);
    }
}

impl Library {
    /// The current snapshot.
    pub fn snapshot(&self) -> Arc<LibrarySnapshot> {
        self.snapshot.clone()
    }
}
