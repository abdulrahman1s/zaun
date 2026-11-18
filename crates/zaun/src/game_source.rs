//! Temporary game source and host context shared by `sandbox print`, `launch`, `stop` and
//! `status` (LAUNCH-006, M6).
//!
//! **Temporary** until the library verbs exist (CLI-002/CLI-003, LIB-*): `<GAME>` is
//!
//! - the path of a Windows executable: a game named after the file, rooted at its folder,
//!   with the default Offline permissions (the setup draft of LOGIC_SPECS §2);
//! - the path of a JSON game fixture (see [`GameFixture`]);
//! - for `launch`/`stop`/`status`: also a game ID, an ID prefix of at least 6 characters, or
//!   a name matching exactly one game in the library.
//!
//! The game ID is derived from the executable's canonical path, so repeated runs use the
//! same prefix. `launch` writes the derived game into the library database (inserting it, or
//! refreshing its settings and fingerprint) so that its sessions have a game row.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use zaun_core::clock::{Clock, SystemClock};
use zaun_core::config::EnvOverrides;
use zaun_core::consts::{INIT_BIN_NAME, LIBEXEC_DIR};
use zaun_core::error::{Error, ErrorCode};
use zaun_core::fsx::{Fs, MountTable, RealFs};
use zaun_core::host::{
    HostFacts, HostProbe, LiveCompositor, LiveHostProbe, ProbeDeps, RealAccess, RealRunner,
};
use zaun_core::ids::{ExecutableId, GameId, RuntimeName};
use zaun_core::jobs::{JobPool, JobPoolConfig};
use zaun_core::launch::HostEnv;
use zaun_core::model::{
    BasePreset, EnvVar, ExeRole, Executable, Fingerprint, Game, GameDirAccess, GameFlags,
    LaunchConfig, PeInfo, RuntimeIsolation, RuntimeRef, Store, sort_name_of,
};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::runtime::{LocalRuntimeService, MemCatalogStore, RuntimeSettings, RuntimeStore};
use zaun_core::sandbox::{self, PolicyIssue};

/// A game defined in a JSON file (M5 test fixture). Paths must be absolute.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameFixture {
    /// Display name; default: the executable's file stem.
    pub name: Option<String>,
    /// The executable.
    pub exe: PathBuf,
    /// Game folder; default: the executable's folder. Ignored with `loose`.
    pub root: Option<PathBuf>,
    /// A loose executable (game folder access `FileOnly`).
    #[serde(default)]
    pub loose: bool,
    /// `offline` (default), `online` or `standard`.
    pub preset: Option<String>,
    /// Launch arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Per-game variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// umu ID (`GAMEID`).
    pub umu_id: Option<String>,
    /// Store (`STORE`).
    pub store: Option<String>,
    /// Compatibility layer: a build name (pinned) or an absolute directory (external);
    /// default: the preference default.
    pub runtime: Option<String>,
    /// Game ID; default: derived from the executable path.
    pub id: Option<String>,
    /// `hardening.runtime_isolation`: `auto` (default), `isolated` or `shared`.
    pub runtime_isolation: Option<String>,
}

pub fn fail(code: ErrorCode, what: impl std::fmt::Display) -> Error {
    Error::new(code).with("detail", what)
}

/// A UUID-shaped string (version 8, "custom") from a hash of `parts`.
fn derived_uuid(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
        h.update([0]);
    }
    let h = h.finalize();
    let hex: String = h[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-8{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

pub fn derived_id(path: &Path) -> GameId {
    derived_uuid(&[b"game", path.as_os_str().as_encoded_bytes()])
        .parse()
        .unwrap_or_else(|_| GameId::from_uuid(Default::default()))
}

pub fn exe_id(game: GameId) -> ExecutableId {
    derived_uuid(&[b"exe", game.to_string().as_bytes()])
        .parse()
        .unwrap_or_else(|_| ExecutableId::from_uuid(Default::default()))
}

fn read_fixture(fs: &dyn Fs, path: &Path) -> Result<GameFixture, Error> {
    let bytes = fs.read(path, 1 << 20)?;
    serde_json::from_slice(&bytes).map_err(|e| fail(ErrorCode::LibInvalidField, e))
}

/// Build the temporary game from an executable path or a fixture.
pub fn load_game(
    fs: &dyn Fs,
    mounts: &MountTable,
    host: &HostFacts,
    clock: &dyn Clock,
    arg: &str,
) -> Result<Game, Error> {
    let arg_path = PathBuf::from(arg);
    let is_fixture = arg_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let fixture = if is_fixture {
        read_fixture(fs, &arg_path)?
    } else {
        GameFixture {
            exe: arg_path,
            ..GameFixture::default()
        }
    };
    if !fixture.exe.is_absolute() {
        return Err(fail(
            ErrorCode::LibInvalidField,
            "the executable path must be absolute",
        ));
    }
    let canonical = fs.canonicalize(&fixture.exe)?;
    let meta = fs.metadata_follow(&canonical)?;
    let not_windows =
        || Error::new(ErrorCode::IdNotWindowsProgram).with("path", canonical.display());
    if !meta.is_file() || fs.read_range(&canonical, 0, 2)? != b"MZ" {
        return Err(not_windows());
    }
    let root = if fixture.loose {
        None
    } else {
        match &fixture.root {
            Some(r) => Some(fs.canonicalize(r)?),
            None => canonical.parent().map(Path::to_path_buf),
        }
    };
    let id = match &fixture.id {
        Some(s) => s.parse().map_err(|e| fail(ErrorCode::LibInvalidField, e))?,
        None => derived_id(&canonical),
    };
    let name = fixture.name.clone().unwrap_or_else(|| {
        canonical
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Game".into())
    });
    let base = match fixture.preset.as_deref() {
        None | Some("offline") => BasePreset::Offline,
        Some("online") => BasePreset::Online,
        Some("standard") => BasePreset::Standard,
        Some(other) => return Err(fail(ErrorCode::LibInvalidField, format!("preset {other}"))),
    };
    let mut policy = sandbox::presets::preset_policy(base, host);
    policy.game_dir = if root.is_some() {
        GameDirAccess::ReadWrite
    } else {
        GameDirAccess::FileOnly
    };
    policy.hardening.runtime_isolation = match fixture.runtime_isolation.as_deref() {
        None | Some("auto") => RuntimeIsolation::Auto,
        Some("isolated") => RuntimeIsolation::Isolated,
        Some("shared") => RuntimeIsolation::Shared,
        Some(other) => {
            return Err(fail(
                ErrorCode::LibInvalidField,
                format!("runtime_isolation {other}"),
            ));
        }
    };
    let runtime = match fixture.runtime.as_deref() {
        None => RuntimeRef::Default,
        Some(r) if r.starts_with('/') => RuntimeRef::External(PathBuf::from(r)),
        Some(r) => RuntimeRef::Pinned(
            RuntimeName::new(r).map_err(|e| fail(ErrorCode::LibInvalidField, e))?,
        ),
    };
    let launch = LaunchConfig {
        args: fixture.args.clone(),
        env: fixture
            .env
            .iter()
            .map(|(k, v)| EnvVar {
                name: k.clone(),
                value: v.clone(),
            })
            .collect(),
        ..LaunchConfig::default()
    };
    let now = clock.now();
    let exe = Executable {
        id: exe_id(id),
        role: ExeRole::Primary,
        path: fixture.exe.clone(),
        rel_path: root
            .as_ref()
            .and_then(|r| canonical.strip_prefix(r).ok().map(Path::to_path_buf)),
        mount_point: mounts
            .mount_point_of(&canonical)
            .map(|m| m.mount_point.clone()),
        canonical_path: canonical,
        fingerprint: Fingerprint {
            size: meta.size,
            mtime_ns: meta.mtime_ns,
            quick_hash: [0; 32],
            full_hash: None,
        },
        pe: PeInfo::default(),
        args_override: None,
        label: None,
        added_at: now,
    };
    let game = Game {
        id,
        sort_name: sort_name_of(&name),
        name,
        name_is_user_set: false,
        root,
        added_at: now,
        updated_at: now,
        favorite: false,
        hidden: false,
        notes: String::new(),
        policy,
        policy_updated_at: now,
        launch,
        umu_id: fixture.umu_id.clone(),
        store: fixture.store.as_deref().map(Store::parse),
        runtime,
        runtime_follow_updates: true,
        runtime_last_resolved: None,
        prefix_tainted_at: None,
        prefix_prepared_with: None,
        flags: GameFlags::default(),
        last_launched_at: None,
        steam_app_id: None,
        steam_app_id_is_user_set: false,
        executables: vec![exe],
    };
    game.validate()
        .map_err(|e| fail(ErrorCode::LibInvalidField, e))?;
    Ok(game)
}

pub(crate) fn group_name(fs: &dyn Fs, gid: u32, fallback: &str) -> String {
    let Ok(bytes) = fs.read_follow(Path::new("/etc/group"), 1 << 20) else {
        return fallback.to_owned();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .find_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            let id = f.nth(1)?.parse::<u32>().ok()?;
            (id == gid).then(|| name.to_owned())
        })
        .unwrap_or_else(|| fallback.to_owned())
}

/// Locate `zaun-init` (INIT-002, DESKTOP_INTEGRATION §10): `ZAUN_INIT`; the path
/// compiled in at build time (`<prefix>/libexec/zaun/zaun-init`); next to the
/// running binary (development builds). `None` when no candidate is an executable file
/// (`ZN-HOST-006`).
pub fn locate_init(over: Option<&Path>, exe_dir: Option<&Path>, fs: &dyn Fs) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = over {
        candidates.push(p.to_path_buf());
    }
    candidates.push(PathBuf::from(LIBEXEC_DIR).join(INIT_BIN_NAME));
    if let Some(dir) = exe_dir {
        candidates.push(dir.join(INIT_BIN_NAME));
    }
    candidates.into_iter().find(|c| {
        fs.metadata_follow(c)
            .is_ok_and(|m| m.is_file() && m.mode & 0o111 != 0)
    })
}

pub(crate) fn init_binary(over: Option<&Path>, fs: &dyn Fs) -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    locate_init(over, exe_dir.as_deref(), fs).unwrap_or_else(|| {
        over.map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(LIBEXEC_DIR).join(INIT_BIN_NAME))
    })
}

#[allow(clippy::print_stderr)]
pub fn print_issues(issues: &[PolicyIssue]) {
    for i in issues {
        let path = i
            .path
            .as_ref()
            .map(|p| format!(" {}", p.display()))
            .unwrap_or_default();
        let msg: &str = &i.message;
        let text = msg.split_once('|').map_or(msg, |(_, t)| t);
        eprintln!("{} {:?}{path}: {text}", i.code.as_str(), i.severity);
    }
}

/// Everything the verbs need about this process and host.
pub struct HostContext {
    /// The environment.
    pub env: PathEnv,
    /// Locations.
    pub paths: AppPaths,
    /// `$HOME`.
    pub home: PathBuf,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Job pool (probe, runtime scans).
    pub jobs: Arc<JobPool>,
    /// Host facts (`ZAUN_BWRAP` replaces the probed bwrap path).
    pub host: HostFacts,
    /// Mount table.
    pub mounts: MountTable,
    /// User name.
    pub user_name: String,
    /// Primary group name.
    pub group: String,
    /// Primary group ID.
    pub gid: u32,
    /// `zaun-init`.
    pub init: PathBuf,
    /// The launcher's environment.
    pub host_env: BTreeMap<String, String>,
}

impl HostContext {
    /// Resolve paths and probe the host.
    pub fn probe() -> Result<Self, Error> {
        let env = PathEnv::from_process();
        let (over, _) = EnvOverrides::from_env(&env);
        let paths = AppPaths::resolve(&env).map_err(|e| fail(ErrorCode::IntTaskFailed, e))?;
        let home = paths
            .home
            .clone()
            .ok_or_else(|| fail(ErrorCode::IntTaskFailed, "HOME is not set"))?;
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
        let jobs = Arc::new(
            JobPool::new(JobPoolConfig::default(), Arc::clone(&clock))
                .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?,
        );
        let user_name = env
            .get("USER")
            .or_else(|| env.get("LOGNAME"))
            .and_then(|u| u.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| "user".into());
        let probe = LiveHostProbe::new(ProbeDeps {
            fs: Arc::clone(&fs),
            runner: Arc::new(RealRunner::new(Arc::clone(&clock))),
            access: Arc::new(RealAccess),
            clock: Arc::clone(&clock),
            env: env.clone(),
            paths: paths.clone(),
            umu_override: over.umu_run.clone(),
            user_name: Some(user_name.clone()),
            jobs: Some(Arc::clone(&jobs)),
            compositor: Arc::new(LiveCompositor),
        });
        let mut host = (*probe.refresh()).clone();
        if let (Some(b), Some(p)) = (host.bwrap.as_mut(), over.bwrap.as_ref()) {
            b.path = p.clone();
        }
        let mounts = MountTable::read(&*fs)?;
        let gid = rustix::process::getgid().as_raw();
        let group = group_name(&*fs, gid, &user_name);
        let init = init_binary(over.init.as_deref(), &*fs);
        let host_env: BTreeMap<String, String> = std::env::vars().collect();
        Ok(Self {
            env,
            paths,
            home,
            clock,
            fs,
            jobs,
            host,
            mounts,
            user_name,
            group,
            gid,
            init,
            host_env,
        })
    }

    /// The runtime service over the store and the user's compatibility tools, scanned now.
    pub fn runtime_service(&self) -> LocalRuntimeService {
        let service = LocalRuntimeService::new(
            Arc::clone(&self.fs),
            Arc::clone(&self.clock),
            Arc::new(MemCatalogStore::new()),
            RuntimeStore::new(&self.paths),
            self.home.clone(),
            RuntimeSettings {
                reflink_ok: self.host.fs.reflink_ok,
                ..RuntimeSettings::default()
            },
            Arc::clone(&self.jobs),
        );
        service.scan_now();
        service
    }

    /// Planning inputs; `install_id` is `meta.install_id` (the stand-in machine ID).
    pub fn host_env<'a>(&'a self, install_id: &'a str) -> HostEnv<'a> {
        HostEnv {
            uid: self.env.uid(),
            gid: self.gid,
            user_name: &self.user_name,
            group_name: &self.group,
            home: &self.home,
            paths: &self.paths,
            host_env: &self.host_env,
            install_id,
            init_binary: &self.init,
            developer_mode: false,
            seccomp_available: cfg!(feature = "seccomp"),
            verbose_log: false,
            global_env: &[],
            user_dirs: &[],
            other_game_roots: &[],
            mounts: &self.mounts,
            fs: &*self.fs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_ids_are_stable_and_distinct() {
        let a = derived_id(Path::new("/g/a.exe"));
        assert_eq!(a, derived_id(Path::new("/g/a.exe")));
        assert_ne!(a, derived_id(Path::new("/g/b.exe")));
        assert_ne!(exe_id(a).to_string(), a.to_string());
        assert_eq!(a.to_string().len(), 36);
    }

    #[test]
    fn fixture_rejects_unknown_fields() {
        let ok: Result<GameFixture, _> =
            serde_json::from_str(r#"{"exe": "/g/a.exe", "args": ["-w"], "preset": "online"}"#);
        assert!(ok.is_ok());
        let bad: Result<GameFixture, _> = serde_json::from_str(r#"{"exe": "/a", "nope": 1}"#);
        assert!(bad.is_err());
    }

    #[test]
    fn init_locator_order() {
        let dir = tempfile::tempdir().unwrap();
        let fs = RealFs::new();
        let over = dir.path().join("over/zaun-init");
        let next = dir.path().join("bin/zaun-init");
        for p in [&over, &next] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "#!/bin/sh\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let bin = dir.path().join("bin");
        assert_eq!(
            locate_init(Some(&over), Some(&bin), &fs),
            Some(over.clone())
        );
        // A missing override falls through (the compiled-in path does not exist in tests).
        let missing = dir.path().join("nope");
        if !Path::new(LIBEXEC_DIR).join(INIT_BIN_NAME).exists() {
            assert_eq!(
                locate_init(Some(&missing), Some(&bin), &fs),
                Some(next.clone())
            );
            assert_eq!(locate_init(None, Some(&missing), &fs), None);
        }
        // Not executable: skipped.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&next, std::fs::Permissions::from_mode(0o644)).unwrap();
        if !Path::new(LIBEXEC_DIR).join(INIT_BIN_NAME).exists() {
            assert_eq!(locate_init(None, Some(&bin), &fs), None);
        }
    }
}
