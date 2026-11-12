//! Building a [`GameDraft`] for an unknown executable: steps 1–7 of LOGIC_SPECS §2. Runs as
//! a job (it reads the file); every later edit in the setup dialog re-validates
//! synchronously with [`GameDraft::revalidate`].

use std::path::{Path, PathBuf};

use super::service::PolicyCheck;
use crate::artwork::IconSource;
use crate::clock::{Clock, Timestamp};
use crate::consts::MIN_PE_SIZE;
use crate::error::{Error, ErrorCode, Result};
use crate::fsx::{Fs, MountTable};
use crate::identity::game_root::{self, RootChoiceError};
use crate::identity::hints::{self, HintInput, SetupHints};
use crate::identity::matcher::LibraryIndex;
use crate::identity::pe::{PeError, PeInspector, PeLimits};
use crate::identity::{GameRootProposal, RootKind, SharedLocations, fingerprint, naming};
use crate::ids::{ExecutableId, GameId, TagName};
use crate::model::{
    EntryKind, ExeRole, Executable, Fingerprint, Game, GameDirAccess, GameFlags, LaunchConfig,
    PeInfo, RuntimeRef, SandboxPolicy, SaveDataLocation,
};
use crate::sandbox::PolicyIssue;

/// The umu-database title match of LOGIC_SPECS §2 step 6 (`umu_match`). `runtime::umu_db`
/// does not exist yet; [`NoUmuTitles`] stands in.
pub trait UmuTitleMatch: Send + Sync {
    /// The umu `GAMEID` whose title equals `name` exactly, if any.
    fn exact_title(&self, name: &str) -> Option<String>;
}

/// No umu database: never matches.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoUmuTitles;

impl UmuTitleMatch for NoUmuTitles {
    fn exact_title(&self, _name: &str) -> Option<String> {
        None
    }
}

/// Everything draft construction reads.
pub struct DraftEnv<'a> {
    /// Filesystem.
    pub fs: &'a dyn Fs,
    /// Clock (bounded listings).
    pub clock: &'a dyn Clock,
    /// PE reader.
    pub pe: &'a dyn PeInspector,
    /// PE limits.
    pub limits: PeLimits,
    /// Icon extraction.
    pub icons: &'a dyn IconSource,
    /// Shared locations (game-root rules).
    pub shared: &'a SharedLocations,
    /// Mount table (stored mount point).
    pub mounts: &'a MountTable,
    /// The library (for "inside a known game root").
    pub index: &'a dyn LibraryIndex,
    /// The preset policy new games start from (`presets::offline()` or the
    /// `default_preset` preference).
    pub base_policy: SandboxPolicy,
    /// umu-database title match.
    pub umu: &'a dyn UmuTitleMatch,
    /// Host-dependent validation.
    pub check: &'a dyn PolicyCheck,
}

/// The setup dialog's model of a new game.
#[derive(Clone, Debug, PartialEq)]
pub struct GameDraft {
    /// The path as opened.
    pub path: PathBuf,
    /// Canonical path (unique across the library).
    pub canonical_path: PathBuf,
    /// Mount point of its filesystem.
    pub mount_point: Option<PathBuf>,
    /// Fingerprint.
    pub fingerprint: Fingerprint,
    /// PE information (possibly unknown).
    pub pe: PeInfo,
    /// Imported DLLs.
    pub imports: Vec<String>,
    /// Name (step 1).
    pub name: String,
    /// Whether the user typed the name.
    pub name_is_user_set: bool,
    /// Game root proposal (step 2).
    pub proposal: GameRootProposal,
    /// The chosen root (`None` = this file only).
    pub root: Option<PathBuf>,
    /// Policy (step 3).
    pub policy: SandboxPolicy,
    /// Launch configuration (step 4).
    pub launch: LaunchConfig,
    /// Compatibility layer (step 4).
    pub runtime: RuntimeRef,
    /// Entry kind, from the default role hint.
    pub kind: EntryKind,
    /// Largest embedded icon as ICO bytes (step 5); `None` = generated placeholder.
    pub icon: Option<Vec<u8>>,
    /// Hints (step 6).
    pub hints: SetupHints,
    /// umu `GAMEID` matched by title (step 6).
    pub umu_id: Option<String>,
    /// Tags typed in the dialog.
    pub tags: Vec<TagName>,
    /// Issues of the current policy (step 7).
    pub issues: Vec<PolicyIssue>,
}

fn not_windows(path: &Path) -> Error {
    Error::new(ErrorCode::IdNotWindowsProgram).with("path", path.display())
}

fn is_msi(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"))
}

/// The `game_dir` kind for a root choice (step 3).
pub fn game_dir_for(root: Option<&Path>) -> GameDirAccess {
    if root.is_some() {
        GameDirAccess::ReadWrite
    } else {
        GameDirAccess::FileOnly
    }
}

/// Steps 1–7 of LOGIC_SPECS §2.
pub fn build_draft(env: &DraftEnv<'_>, path: &Path) -> Result<GameDraft> {
    let canonical = env.fs.canonicalize(path)?;
    let md = env.fs.metadata(&canonical)?;
    if !md.is_file() || md.size < MIN_PE_SIZE {
        return Err(not_windows(path));
    }
    let fingerprint = fingerprint::compute(env.fs, &canonical)?;
    let pe = match env.pe.inspect(&canonical, &env.limits) {
        Ok(info) => info,
        Err(PeError::NotExecutable | PeError::NotPe(_)) if !is_msi(&canonical) => {
            return Err(not_windows(path));
        }
        Err(e) => {
            tracing::info!(error = %e, "PE information unavailable; continuing without");
            PeInfo::unknown()
        }
    };
    let imports = env.pe.imports(&canonical, &env.limits).unwrap_or_default();
    let proposal = game_root::detect(env.fs, env.shared, &canonical)?;
    let root = proposal.root.clone();
    let exe_dir = canonical.parent().map(Path::to_path_buf);
    let mut siblings = exe_dir
        .as_deref()
        .map(|d| hints::list_names(env.fs, env.clock, d))
        .unwrap_or_default();
    if let Some(r) = &root
        && Some(r) != exe_dir.as_ref()
    {
        siblings.extend(hints::list_names(env.fs, env.clock, r));
    }
    let file_name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let inside_known_root = env
        .index
        .roots()
        .iter()
        .any(|(r, _)| canonical.starts_with(r));
    let hints = hints::setup_hints(&HintInput {
        file_name: &file_name,
        size: fingerprint.size,
        pe: &pe,
        imports: &imports,
        siblings: &siblings,
        loose: proposal.kind == RootKind::LooseFile,
        inside_known_root,
    });
    let name = naming::derive_name(&pe, root.as_deref(), &canonical);
    let mut policy = env.base_policy.clone();
    policy.game_dir = game_dir_for(root.as_deref());
    policy.save_data = SaveDataLocation::Internal;
    let kind = match hints.default_role {
        Some(ExeRole::Installer) => EntryKind::Installer,
        Some(ExeRole::Tool) => EntryKind::Tool,
        _ => EntryKind::Game,
    };
    let icon = match env.icons.largest_icon(&canonical) {
        Ok(i) => i,
        Err(e) => {
            tracing::info!(code = %e.code, "no icon extracted");
            None
        }
    };
    let umu_id = env.umu.exact_title(&name);
    let mount_point = env
        .mounts
        .mount_point_of(&canonical)
        .map(|m| m.mount_point.clone());
    let mut draft = GameDraft {
        path: path.to_path_buf(),
        canonical_path: canonical,
        mount_point,
        fingerprint,
        pe,
        imports,
        name,
        name_is_user_set: false,
        proposal,
        root,
        policy,
        launch: LaunchConfig::default(),
        runtime: RuntimeRef::Default,
        kind,
        icon,
        hints,
        umu_id,
        tags: Vec::new(),
        issues: Vec::new(),
    };
    draft.revalidate(
        env.check,
        &env.index
            .roots()
            .into_iter()
            .map(|(r, _)| r)
            .collect::<Vec<_>>(),
    );
    Ok(draft)
}

impl GameDraft {
    /// The user typed a name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
        self.name_is_user_set = true;
    }

    /// Choose another root among the proposal's candidates or "this file only" (`None`);
    /// the `game_dir` kind follows the loose-file rule.
    pub fn choose_root(
        &mut self,
        shared: &SharedLocations,
        root: Option<PathBuf>,
    ) -> std::result::Result<(), RootChoiceError> {
        if let Some(r) = &root {
            game_root::check_choice(shared, &self.canonical_path, r)?;
        }
        let was_file_only = self.policy.game_dir == GameDirAccess::FileOnly;
        if root.is_none() {
            self.policy.game_dir = GameDirAccess::FileOnly;
        } else if was_file_only {
            self.policy.game_dir = GameDirAccess::ReadWrite;
        }
        self.root = root;
        Ok(())
    }

    /// Recompute the issues (every edit; pure apart from what `check` reads).
    pub fn revalidate(&mut self, check: &dyn PolicyCheck, other_roots: &[PathBuf]) {
        let game = self.to_game(
            GameId::from_uuid(uuid::Uuid::nil()),
            ExecutableId::from_uuid(uuid::Uuid::nil()),
            Timestamp::EPOCH,
        );
        self.issues = check.issues(&game, other_roots);
    }

    /// Whether an Error-severity issue blocks confirming.
    pub fn is_blocked(&self) -> bool {
        crate::sandbox::validate::any_blocking(&self.issues)
    }

    /// The game this draft describes.
    pub fn to_game(&self, id: GameId, exe: ExecutableId, now: Timestamp) -> Game {
        let rel_path = self
            .root
            .as_ref()
            .and_then(|r| self.canonical_path.strip_prefix(r).ok())
            .map(Path::to_path_buf);
        let name = self.name.trim().to_owned();
        Game {
            id,
            sort_name: naming::sort_name(&name),
            name,
            name_is_user_set: self.name_is_user_set,
            root: self.root.clone(),
            added_at: now,
            updated_at: now,
            favorite: false,
            hidden: false,
            notes: String::new(),
            policy: self.policy.clone(),
            policy_updated_at: now,
            launch: self.launch.clone(),
            umu_id: self.umu_id.clone(),
            store: None,
            runtime: self.runtime.clone(),
            runtime_follow_updates: true,
            runtime_last_resolved: None,
            prefix_tainted_at: None,
            prefix_prepared_with: None,
            flags: super::with_entry_kind(GameFlags::empty(), self.kind),
            last_launched_at: None,
            steam_app_id: None,
            steam_app_id_is_user_set: false,
            executables: vec![Executable {
                id: exe,
                role: ExeRole::Primary,
                path: self.path.clone(),
                canonical_path: self.canonical_path.clone(),
                rel_path,
                mount_point: self.mount_point.clone(),
                fingerprint: self.fingerprint,
                pe: self.pe.clone(),
                args_override: None,
                label: None,
                added_at: now,
            }],
        }
    }
}
