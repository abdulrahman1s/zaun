//! `easyplay sandbox print <GAME> [--format args|json|explain|shell] [--kind game|prepare]`
//! (LAUNCH-006, CONFIGURATION §6, DEVELOPER_MODE §6).
//!
//! A dry run: nothing is created, no prefix is marked, no session row is written. Until the
//! library database is wired to the CLI (CLI-002), `<GAME>` is a temporary game source:
//!
//! - the path of a Windows executable: a game named after the file, rooted at its folder,
//!   with the default Offline permissions (the setup draft of LOGIC_SPECS §2);
//! - the path of a JSON game fixture (see [`GameFixture`]).
//!
//! The game ID is derived from the executable's canonical path, so repeated prints use the
//! same prefix. When the game's compatibility layer still has to be installed, the plan of
//! the Provision step is printed with a note, as a dry run in the GUI would.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::config::EnvOverrides;
use easyplay_core::consts::{INIT_BIN_NAME, LIBEXEC_DIR};
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::fsx::{Fs, MountTable, RealFs};
use easyplay_core::host::{HostFacts, HostProbe, LiveHostProbe, ProbeDeps, RealAccess, RealRunner};
use easyplay_core::ids::{ExecutableId, GameId, RuntimeName, SessionId};
use easyplay_core::jobs::{JobPool, JobPoolConfig};
use easyplay_core::launch::preflight::STAT_DEADLINE;
use easyplay_core::launch::{
    self, HostEnv, LaunchTarget, NoLocks, PlanRequest, PlannedLaunch, PreflightInput,
};
use easyplay_core::model::{
    BasePreset, EnvVar, ExeRole, Executable, Fingerprint, Game, GameDirAccess, GameFlags,
    LaunchConfig, PeInfo, ResolvedRuntime, RuntimeIsolation, RuntimeRef, SessionKind, Store,
    sort_name_of,
};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::runtime::{
    LocalRuntimeService, MemCatalogStore, Resolution, RuntimeService, RuntimeSettings, RuntimeStore,
};
use easyplay_core::sandbox::{self, PolicyIssue, ValidationInputs};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::cli::{PlanKind, SandboxFormat};

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

fn fail(code: ErrorCode, what: impl std::fmt::Display) -> Error {
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

fn derived_id(path: &Path) -> GameId {
    derived_uuid(&[b"game", path.as_os_str().as_encoded_bytes()])
        .parse()
        .unwrap_or_else(|_| GameId::from_uuid(Default::default()))
}

fn exe_id(game: GameId) -> ExecutableId {
    derived_uuid(&[b"exe", game.to_string().as_bytes()])
        .parse()
        .unwrap_or_else(|_| ExecutableId::from_uuid(Default::default()))
}

fn read_fixture(fs: &dyn Fs, path: &Path) -> Result<GameFixture, Error> {
    let bytes = fs.read(path, 1 << 20)?;
    serde_json::from_slice(&bytes).map_err(|e| fail(ErrorCode::LibInvalidField, e))
}

/// Build the temporary game.
fn load_game(
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

fn group_name(fs: &dyn Fs, gid: u32, fallback: &str) -> String {
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

fn init_binary(over: Option<&Path>, fs: &dyn Fs) -> PathBuf {
    if let Some(p) = over {
        return p.to_path_buf();
    }
    let mut candidates = vec![PathBuf::from(LIBEXEC_DIR).join(INIT_BIN_NAME)];
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        candidates.push(dir.join(INIT_BIN_NAME));
    }
    candidates
        .iter()
        .find(|c| fs.metadata_follow(c).is_ok_and(|m| m.is_file()))
        .unwrap_or(&candidates[0])
        .clone()
}

#[allow(clippy::print_stderr)]
fn print_issues(issues: &[PolicyIssue]) {
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

/// Run the verb.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn run(game_arg: &str, format: SandboxFormat, kind: PlanKind) -> Result<(), Error> {
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
    });
    let mut host = (*probe.refresh()).clone();
    if let (Some(b), Some(p)) = (host.bwrap.as_mut(), over.bwrap.as_ref()) {
        b.path = p.clone();
    }
    let mounts = MountTable::read(&*fs)?;
    let game = load_game(&*fs, &mounts, &host, &*clock, game_arg)?;

    let service = LocalRuntimeService::new(
        Arc::clone(&fs),
        Arc::clone(&clock),
        Arc::new(MemCatalogStore::new()),
        RuntimeStore::new(&paths),
        home.clone(),
        RuntimeSettings {
            reflink_ok: host.fs.reflink_ok,
            ..RuntimeSettings::default()
        },
        Arc::clone(&jobs),
    );
    service.scan_now();
    let resolution = service.resolve(&game.runtime, Some(&game));
    let mut notes: Vec<String> = Vec::new();
    let (target, runtime, codename): (LaunchTarget<'_>, Option<ResolvedRuntime>, Option<String>) =
        match (resolution, kind) {
            (Resolution::Ready(rt), PlanKind::Game) => (
                LaunchTarget::Game {
                    exe: &game.executables[0],
                    extra_args: &[],
                },
                Some(rt),
                None,
            ),
            (Resolution::Ready(rt), PlanKind::Prepare) => (LaunchTarget::Prepare, Some(rt), None),
            (Resolution::NeedsProvision(need), _) => {
                notes.push(format!(
                    "The compatibility layer is not installed yet ({}); this is the plan of \
                     the download step that would run first.",
                    need.protonpath()
                ));
                (LaunchTarget::Provision, None, Some(need.protonpath()))
            }
            (Resolution::Unavailable(r), _) => {
                return Err(Error::new(r.code()).with("reason", format!("{r:?}")));
            }
        };

    let gid = rustix::process::getgid().as_raw();
    let group = group_name(&*fs, gid, &user_name);
    let init = init_binary(over.init.as_deref(), &*fs);
    let host_env: BTreeMap<String, String> = std::env::vars().collect();
    let host_env_ref = HostEnv {
        uid: env.uid(),
        gid,
        user_name: &user_name,
        group_name: &group,
        home: &home,
        paths: &paths,
        host_env: &host_env,
        // No library database yet (CLI-002): the Provision stand-in machine ID uses zeros.
        install_id: "00000000000000000000000000000000",
        init_binary: &init,
        developer_mode: false,
        seccomp_available: cfg!(feature = "seccomp"),
        verbose_log: false,
        global_env: &[],
        user_dirs: &[],
        other_game_roots: &[],
        mounts: &mounts,
        fs: &*fs,
    };
    let session_id = SessionId::generate(&*clock);
    let req = PlanRequest {
        session_id,
        target,
        game: Some(&game),
        runtime: runtime.as_ref(),
        proton_codename: codename.as_deref(),
        network_override: None,
        host: &host,
        env: &host_env_ref,
    };

    // Preflight for game launches: a dry run reports its failures the same way.
    if target.kind() == SessionKind::Game {
        let prefix_lock = paths.prefix_lock(game.id);
        let checked = launch::with_context(&req, |ctx, _, _| {
            launch::preflight(&PreflightInput {
                game: &game,
                exe: &game.executables[0],
                kind: SessionKind::Game,
                host: &host,
                validation: ValidationInputs::from_context(ctx, &host),
                fs: Arc::clone(&fs),
                mounts: &mounts,
                init_binary: &init,
                prefix_lock: &prefix_lock,
                locks: &NoLocks,
                data_dir: &paths.data_dir,
                isolation: runtime.as_ref().map(|r| (r.isolation, host.fs.reflink_ok)),
                deadline: STAT_DEADLINE,
            })
        })
        .map_err(Error::from)?;
        match checked {
            Ok(report) => {
                if let Some((avail, wanted)) = report.low_space {
                    notes.push(format!(
                        "Low disk space: {} MiB free, {} MiB recommended (EP-FS-002).",
                        avail >> 20,
                        wanted >> 20
                    ));
                }
            }
            Err(f) => {
                if let launch::PreflightFailure::PolicyInvalid(issues) = &f {
                    print_issues(issues);
                }
                return Err(f.into());
            }
        }
    }

    let planned: PlannedLaunch = match launch::plan_launch(&req) {
        Ok(p) => p,
        Err(e) => {
            print_issues(e.issues());
            return Err(e.into());
        }
    };
    for n in &notes {
        eprintln!("note: {n}");
    }
    for n in &planned.sandbox.notes {
        eprintln!("note: {}", n.text);
    }
    print_issues(&planned.sandbox.issues);

    match format {
        SandboxFormat::Args => {
            println!("{}", launch::shell::quote(planned.plan.bwrap.as_os_str()));
            for g in &planned.rendered.groups {
                let line: Vec<String> = g.iter().map(|a| launch::shell::quote(a)).collect();
                println!("{}", line.join(" "));
            }
            let cmd: Vec<String> = planned
                .rendered
                .command
                .iter()
                .map(|a| launch::shell::quote(a))
                .collect();
            println!("-- {}", cmd.join(" "));
        }
        SandboxFormat::Json => {
            #[derive(serde::Serialize)]
            struct Out<'a> {
                plan: &'a launch::LaunchPlan,
                sandbox: &'a sandbox::SandboxPlan,
                umu: &'a easyplay_core::umu::UmuInvocation,
                notes: &'a [String],
            }
            let out = Out {
                plan: &planned.plan,
                sandbox: &planned.sandbox,
                umu: &planned.umu,
                notes: &notes,
            };
            let text = serde_json::to_string_pretty(&out)
                .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?;
            println!("{text}");
        }
        SandboxFormat::Explain => {
            for s in sandbox::explain(&planned.sandbox, &game.policy) {
                println!("[{:?}] {}", s.severity, s.text);
                if let Some(d) = &s.detail {
                    println!("    {d}");
                }
            }
        }
        SandboxFormat::Shell => {
            print!(
                "{}",
                launch::render_shell(&planned.plan, &clock.now().to_rfc3339())
            );
        }
    }
    Ok(())
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
}
