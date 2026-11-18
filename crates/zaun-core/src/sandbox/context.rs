//! `SandboxContext`: everything about one launch that is not the policy or the host facts
//! (BUBBLEWRAP_SANDBOX §2).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::fsx::{Fs, MountTable};
use crate::host::UmuFacts;
use crate::ids::{GameId, SessionId};
use crate::model::{
    CompatOptions, EnvVar, NetworkAccess, ResolvedRuntime, SessionKind, UserDir, WorkingDir,
};
use crate::paths::AppPaths;

/// Inputs of one planning call besides the policy and `HostFacts`.
///
/// Fields beyond BUBBLEWRAP_SANDBOX §2 (documented there as additions): `group_name`,
/// `paths`, `launch`, `host_env`, `install_id`, `developer_mode`, `seccomp_available`,
/// `proton_codename`, `user_dirs`, `other_game_roots`, `mounts` and `fs`. `runtime` is an
/// `Option` because a Provision session may resolve only a codename.
#[derive(Clone, Copy)]
pub struct SandboxContext<'a> {
    /// Session kind.
    pub kind: SessionKind,
    /// User ID (mapped to itself).
    pub uid: u32,
    /// Primary group ID.
    pub gid: u32,
    /// User name (synthesized `passwd`, `USER`, `LOGNAME`).
    pub user_name: &'a str,
    /// Primary group name (synthesized `group`).
    pub group_name: &'a str,
    /// Real `$HOME` path, used as the in-sandbox `HOME`.
    pub home: &'a Path,
    /// The game; `None` for Provision.
    pub game: Option<GameContext<'a>>,
    /// The resolved runtime; required for every kind except Provision.
    pub runtime: Option<&'a ResolvedRuntime>,
    /// `PROTONPATH` codename or build name for trusted network sessions that download.
    pub proton_codename: Option<&'a str>,
    /// Per-session directories.
    pub session: &'a SessionDirs,
    /// Zaun's locations (runtime store, deny-list entries).
    pub paths: &'a AppPaths,
    /// Host path of `zaun-init`.
    pub init_binary: &'a Path,
    /// The `umu-run` to invoke (normally `HostFacts.umu`).
    pub umu: &'a UmuFacts,
    /// Network override for Prepare, Provision and tools with consent.
    pub network_override: Option<NetworkAccess>,
    /// Launch configuration inputs.
    pub launch: LaunchInputs<'a>,
    /// The launcher's environment; only allow-listed names are copied (UMU_INTEGRATION §4).
    pub host_env: &'a BTreeMap<String, String>,
    /// `meta.install_id` (random 128-bit hex): the Provision stand-in machine ID.
    pub install_id: &'a str,
    /// Developer Mode is on.
    pub developer_mode: bool,
    /// A seccomp program can be built in this build (feature `seccomp`).
    pub seccomp_available: bool,
    /// XDG user directories (breadth warning for game folders).
    pub user_dirs: &'a [(UserDir, PathBuf)],
    /// Root folders of the other games in the library.
    pub other_game_roots: &'a [PathBuf],
    /// Parsed `/proc/self/mountinfo`.
    pub mounts: &'a MountTable,
    /// Read-only filesystem access (listing the runtime, canonicalising grants).
    pub fs: &'a dyn Fs,
}

impl SandboxContext<'_> {
    /// Whether this is a trusted session with the network on (Provision; Prepare with the
    /// network granted by override): runtime store and umu cache read-write.
    pub fn trusted_network(&self) -> bool {
        match self.kind {
            SessionKind::Provision => self.network_override != Some(NetworkAccess::None),
            SessionKind::Prepare => self.network_override == Some(NetworkAccess::Full),
            _ => false,
        }
    }

    /// Whether the policy's game-facing fields apply (game folder, grants, devices, audio).
    pub fn untrusted(&self) -> bool {
        self.kind.is_untrusted()
    }
}

/// Paths of the game, all canonical host paths.
#[derive(Clone, Copy, Debug)]
pub struct GameContext<'a> {
    /// Game ID.
    pub id: GameId,
    /// `…/games/<id>/home`.
    pub private_home: &'a Path,
    /// `…/games/<id>/prefix`.
    pub prefix: &'a Path,
    /// `…/games/<id>/overlay/upper`.
    pub overlay_upper: &'a Path,
    /// `…/games/<id>/overlay/work`.
    pub overlay_work: &'a Path,
    /// Game folder; `None` for loose executables.
    pub root: Option<&'a Path>,
    /// Executable.
    pub exe: &'a Path,
    /// `GAMEID` (umu ID), default `umu-default`.
    pub umu_id: Option<&'a str>,
    /// `STORE`.
    pub store: Option<&'a str>,
    /// `games/<id>/session.lock`.
    pub prefix_lock: &'a Path,
}

/// Launch-configuration inputs (DATA_MODEL §4).
#[derive(Clone, Copy, Debug)]
pub struct LaunchInputs<'a> {
    /// Working directory choice.
    pub working_dir: &'a WorkingDir,
    /// Compatibility options.
    pub compat: &'a CompatOptions,
    /// Global user variables (preferences).
    pub global_env: &'a [EnvVar],
    /// Per-game user variables.
    pub game_env: &'a [EnvVar],
    /// Arguments after `umu-run` (from `umu::command`).
    pub umu_args: &'a [OsString],
    /// Verbose launch logging (`UMU_LOG=1`).
    pub verbose_log: bool,
}

/// Per-session directories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionDirs {
    /// Session ID.
    pub id: SessionId,
    /// Session log directory (`<state>/logs/sessions/<sid>`).
    pub log_dir: PathBuf,
    /// Pressure-vessel variable directory (`<store>/session-var/<sid>`).
    pub var_dir: PathBuf,
    /// Session runtime directory (`<run>/sessions/<sid>`), removed when the session ends.
    pub run_dir: PathBuf,
}

impl SessionDirs {
    /// The directories of session `id`.
    pub fn new(paths: &AppPaths, id: SessionId) -> Self {
        Self {
            id,
            log_dir: paths.session_dir(id),
            var_dir: paths.session_var_dir(id),
            run_dir: paths.run_session_dir(id),
        }
    }
}

/// Owned paths of one game, for building a [`GameContext`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GamePaths {
    /// Private home.
    pub private_home: PathBuf,
    /// Prefix.
    pub prefix: PathBuf,
    /// Overlay upper layer.
    pub overlay_upper: PathBuf,
    /// Overlay work directory.
    pub overlay_work: PathBuf,
    /// Prefix lock.
    pub prefix_lock: PathBuf,
}

impl GamePaths {
    /// The standard locations of game `id`.
    pub fn new(paths: &AppPaths, id: GameId) -> Self {
        Self {
            private_home: paths.home_dir(id),
            prefix: paths.prefix_dir(id),
            overlay_upper: paths.overlay_upper_dir(id),
            overlay_work: paths.overlay_work_dir(id),
            prefix_lock: paths.prefix_lock(id),
        }
    }
}
