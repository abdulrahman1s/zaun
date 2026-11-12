//! Flow tests of the launch orchestrator (LAUNCH-004, M6 scope) with `FakeLauncher`,
//! `FakeClock` and scripted runtime resolutions: the states of LOGIC_SPECS §3 in order and
//! the failure rows reachable without Provision/Prepare.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use easyplay_core::clock::{Clock, Timestamp};
use easyplay_core::error::ErrorCode;
use easyplay_core::fsx::{DirEntry, Fs, FsResult, Metadata};
use easyplay_core::ids::SessionId;
use easyplay_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use easyplay_core::launch::{
    self, HostEnv, LaunchError, LaunchPrompt, LaunchRequest, LaunchState, NoLocks,
    OrchestratorDeps, PrefixLocks,
};
use easyplay_core::model::{
    ExeRole, Executable, Fingerprint, Game, GameFlags, LaunchConfig, PeInfo, RuntimeRef,
};
use easyplay_core::runtime::{
    ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService, UnavailableReason,
};
use easyplay_core::sandbox;
use easyplay_testkit::fixtures::hosts::{self, SandboxWorld};
use easyplay_testkit::{FakeClock, FakeLauncher, FakeStart};

struct WorldFs(Arc<SandboxWorld>);

macro_rules! delegate {
    ($($name:ident($($a:ident: $t:ty),*) -> $r:ty;)*) => {
        impl Fs for WorldFs {
            $(fn $name(&self, $($a: $t),*) -> $r { self.0.fs.$name($($a),*) })*
        }
    };
}

delegate! {
    metadata(p: &Path) -> FsResult<Metadata>;
    metadata_follow(p: &Path) -> FsResult<Metadata>;
    read_link(p: &Path) -> FsResult<PathBuf>;
    read(p: &Path, l: u64) -> FsResult<Vec<u8>>;
    read_follow(p: &Path, l: u64) -> FsResult<Vec<u8>>;
    read_range(p: &Path, o: u64, l: usize) -> FsResult<Vec<u8>>;
    open_read(p: &Path) -> FsResult<Box<dyn std::io::Read + Send>>;
    read_dir(p: &Path) -> FsResult<Vec<DirEntry>>;
    canonicalize(p: &Path) -> FsResult<PathBuf>;
    create_dir(p: &Path, m: u32) -> FsResult<()>;
    create_dir_all(p: &Path, m: u32) -> FsResult<()>;
    write_new(p: &Path, d: &[u8], m: u32) -> FsResult<()>;
    atomic_write(p: &Path, d: &[u8], m: u32) -> FsResult<()>;
    rename(a: &Path, b: &Path) -> FsResult<()>;
    remove_file(p: &Path) -> FsResult<()>;
    remove_dir(p: &Path) -> FsResult<()>;
    symlink(a: &Path, b: &Path) -> FsResult<()>;
    set_mode(p: &Path, m: u32) -> FsResult<()>;
    free_space(p: &Path) -> FsResult<u64>;
    reflink_supported(a: &Path, b: &Path) -> FsResult<bool>;
}

/// A runtime service answering with a scripted resolution.
struct Scripted {
    resolution: Resolution,
    pool: JobPool,
}

impl RuntimeService for Scripted {
    fn catalog(&self) -> Arc<RuntimeCatalog> {
        Arc::new(RuntimeCatalog::default())
    }
    fn refresh(&self) -> JobHandle<Arc<RuntimeCatalog>> {
        self.pool
            .submit(JobPriority::Normal, JobLabel("refresh"), |_| {
                Arc::new(RuntimeCatalog::default())
            })
    }
    fn resolve(&self, _: &RuntimeRef, _: Option<&Game>) -> Resolution {
        self.resolution.clone()
    }
}

struct Held;
impl PrefixLocks for Held {
    fn is_held(&self, _: &Path) -> bool {
        true
    }
}

fn world() -> Arc<SandboxWorld> {
    let w = SandboxWorld::worked_example();
    w.fs.file(&w.init, b"\x7fELF");
    w.fs.set_mode(&w.init, 0o755).unwrap();
    w.fs.set_free_space(100 << 30);
    Arc::new(w)
}

fn game_of(w: &SandboxWorld) -> Game {
    let meta = w.fs.metadata_follow(&w.exe).unwrap();
    let now = Timestamp::from_millis(1_794_480_000_000);
    Game {
        id: w.game_id,
        name: "Hollow Knight".into(),
        sort_name: "hollow knight".into(),
        name_is_user_set: false,
        root: w.root.clone(),
        added_at: now,
        updated_at: now,
        favorite: false,
        hidden: false,
        notes: String::new(),
        policy: sandbox::offline(),
        policy_updated_at: now,
        launch: LaunchConfig::default(),
        umu_id: None,
        store: None,
        runtime: RuntimeRef::Default,
        runtime_follow_updates: true,
        runtime_last_resolved: None,
        prefix_tainted_at: None,
        prefix_prepared_with: None,
        flags: GameFlags::default(),
        last_launched_at: None,
        steam_app_id: None,
        steam_app_id_is_user_set: false,
        executables: vec![Executable {
            id: "0192f0c2-1c3a-7d4e-8f00-0000000000e1".parse().unwrap(),
            role: ExeRole::Primary,
            path: w.exe.clone(),
            canonical_path: w.exe.clone(),
            rel_path: None,
            mount_point: Some("/mnt/games".into()),
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
        }],
    }
}

fn env_of(w: &SandboxWorld) -> HostEnv<'_> {
    HostEnv {
        uid: 1000,
        gid: 1000,
        user_name: "sam",
        group_name: "sam",
        home: Path::new(hosts::HOME),
        paths: &w.paths,
        host_env: &w.host_env,
        install_id: hosts::INSTALL_ID,
        init_binary: &w.init,
        developer_mode: false,
        seccomp_available: true,
        verbose_log: false,
        global_env: &[],
        user_dirs: &w.user_dirs,
        other_game_roots: &w.other_roots,
        mounts: &w.mounts,
        fs: &w.fs,
    }
}

struct Outcome {
    result: Result<launch::LaunchOutcome, LaunchError>,
    states: Vec<&'static str>,
}

fn run(
    w: &Arc<SandboxWorld>,
    game: &Game,
    resolution: Resolution,
    launcher: &FakeLauncher,
    locks: &dyn PrefixLocks,
) -> Outcome {
    let clock = FakeClock::baseline();
    let runtimes = Scripted {
        resolution,
        pool: JobPool::new(JobPoolConfig::default(), Arc::new(FakeClock::baseline())).unwrap(),
    };
    let env = env_of(w);
    let deps = OrchestratorDeps {
        host: &w.host,
        env: &env,
        runtimes: &runtimes,
        launcher,
        locks,
        fs: Arc::new(WorldFs(Arc::clone(w))),
        clock: &clock,
    };
    let req = LaunchRequest {
        game,
        exe: None,
        extra_args: &[],
        session_id: SessionId::generate(&clock as &dyn Clock),
    };
    let mut states = Vec::new();
    let result = launch::launch(&req, &deps, &mut |s: &LaunchState| {
        states.push(s.kind().as_str())
    });
    Outcome { result, states }
}

fn code(o: Outcome) -> ErrorCode {
    match o.result {
        Err(LaunchError::Failed(e)) => e.code,
        other => panic!("expected a failure, got {:?}", other.map(|o| o.handle)),
    }
}

#[test]
fn ready_runtime_reaches_starting() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let o = run(&w, &g, Resolution::Ready(w.runtime.clone()), &l, &NoLocks);
    assert_eq!(
        o.states,
        [
            "validating",
            "resolving_runtime",
            "planning",
            "spawning",
            "starting"
        ]
    );
    let out = o.result.unwrap();
    let plans = l.plans();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].session.id, out.handle.session_id);
    assert_eq!(plans[0].session.game_id, Some(g.id));
    assert_eq!(out.planned.plan, plans[0]);
    assert_eq!(out.launch.state().kind().as_str(), "starting");
}

#[test]
fn prefix_lock_held_is_already_running() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    assert_eq!(
        code(run(&w, &g, Resolution::Ready(w.runtime.clone()), &l, &Held)),
        ErrorCode::SesAlreadyRunning
    );
    // Also without a runtime (the lock is checked first).
    let need = Resolution::NeedsProvision(ProvisionNeed::Codename(
        easyplay_core::model::Channel::UmuProton,
    ));
    assert_eq!(
        code(run(&w, &g, need, &l, &Held)),
        ErrorCode::SesAlreadyRunning
    );
    assert!(l.plans().is_empty());
}

#[test]
fn provision_needed_stops_before_planning() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let need = Resolution::NeedsProvision(ProvisionNeed::Codename(
        easyplay_core::model::Channel::UmuProton,
    ));
    let o = run(&w, &g, need, &l, &NoLocks);
    assert_eq!(o.states, ["validating", "resolving_runtime"]);
    assert!(matches!(o.result, Err(LaunchError::NeedsProvision(_))));
    assert!(l.plans().is_empty());
}

#[test]
fn unavailable_runtime() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let r = Resolution::Unavailable(UnavailableReason::ExternalInvalid("/opt/proton".into()));
    assert_eq!(
        code(run(&w, &g, r, &l, &NoLocks)),
        ErrorCode::RtNoLongerInstalled
    );
}

#[test]
fn missing_executable() {
    let w = world();
    let mut g = game_of(&w);
    g.executables[0].canonical_path = "/mnt/games/Hollow Knight/gone.exe".into();
    g.executables[0].path = g.executables[0].canonical_path.clone();
    let l = FakeLauncher::new();
    let o = run(&w, &g, Resolution::Ready(w.runtime.clone()), &l, &NoLocks);
    assert_eq!(o.states, ["validating"]);
    assert_eq!(code(o), ErrorCode::LibExeNotFound);
}

#[test]
fn changed_executable_raises_a_prompt() {
    let w = world();
    let mut g = game_of(&w);
    g.executables[0].fingerprint.size += 1;
    let l = FakeLauncher::new();
    let o = run(&w, &g, Resolution::Ready(w.runtime.clone()), &l, &NoLocks);
    assert!(matches!(
        o.result,
        Err(LaunchError::Prompt(LaunchPrompt::ExeChanged { .. }))
    ));
    assert!(l.plans().is_empty());
}

#[test]
fn host_requirement_missing() {
    let w = world();
    let g = game_of(&w);
    let mut host = w.host.clone();
    host.umu = None;
    let mut w2 = SandboxWorld::worked_example();
    w2.host = host;
    w2.fs.file(&w2.init, b"\x7fELF");
    w2.fs.set_mode(&w2.init, 0o755).unwrap();
    w2.fs.set_free_space(100 << 30);
    let w2 = Arc::new(w2);
    let l = FakeLauncher::new();
    let c = code(run(
        &w2,
        &g,
        Resolution::Ready(w.runtime.clone()),
        &l,
        &NoLocks,
    ));
    assert_eq!(c, ErrorCode::HostUmuNotFound);
}

#[test]
fn supervisor_not_started() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    l.push(FakeStart::Fails(ErrorCode::SesSupervisorNotStarted));
    let o = run(&w, &g, Resolution::Ready(w.runtime.clone()), &l, &NoLocks);
    assert_eq!(
        o.states,
        ["validating", "resolving_runtime", "planning", "spawning"]
    );
    assert_eq!(code(o), ErrorCode::SesSupervisorNotStarted);
    assert_eq!(l.plans().len(), 1);
}
