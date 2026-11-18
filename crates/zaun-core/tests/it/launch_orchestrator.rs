//! Flow tests of the launch orchestrator (LAUNCH-004, LAUNCH-005) with `FakeLauncher`,
//! `FakeClock`, scripted runtime resolutions and `ScriptedSteps`: the states of LOGIC_SPECS
//! §3 in order, every branch of RUNTIME_MANAGEMENT §4.3, the audit gate with a quarantine,
//! the pristine-prefix rule, and dry runs without side effects.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use zaun_core::clock::{Clock, Timestamp};
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{DirEntry, Fs, FsResult, Metadata};
use zaun_core::ids::SessionId;
use zaun_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use zaun_core::launch::{
    self, DryStep, HostEnv, LaunchError, LaunchPrompt, LaunchRequest, LaunchState, NoLocks,
    OrchestratorDeps, PrefixLocks, TrustedSteps,
};
use zaun_core::model::{
    ExeRole, Executable, Fingerprint, Game, GameFlags, LaunchConfig, PeInfo, RuntimeRef,
};
use zaun_core::runtime::{
    ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService, UnavailableReason,
};
use zaun_core::sandbox;
use zaun_testkit::fakes::{ScriptedSteps, StepScript};
use zaun_testkit::fixtures::hosts::{self, SandboxWorld};
use zaun_testkit::{FakeClock, FakeLauncher, FakeStart};

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

/// A runtime service answering with a scripted resolution (changed by step hooks).
struct Scripted {
    resolution: Arc<Mutex<Resolution>>,
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
        self.resolution.lock().clone()
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
    run_with(
        w,
        game,
        &Arc::new(Mutex::new(resolution)),
        launcher,
        locks,
        None,
        false,
    )
}

fn run_with(
    w: &Arc<SandboxWorld>,
    game: &Game,
    resolution: &Arc<Mutex<Resolution>>,
    launcher: &FakeLauncher,
    locks: &dyn PrefixLocks,
    steps: Option<&ScriptedSteps>,
    consent: bool,
) -> Outcome {
    let clock = FakeClock::baseline();
    let runtimes = Scripted {
        resolution: Arc::clone(resolution),
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
        steps: steps.map(|s| s as &dyn TrustedSteps),
        cancel: None,
    };
    let req = LaunchRequest {
        game,
        exe: None,
        extra_args: &[],
        session_id: SessionId::generate(&clock as &dyn Clock),
        network_consent: consent,
    };
    let mut states = Vec::new();
    // Progress updates within a state are collapsed.
    let result = launch::launch(&req, &deps, &mut |s: &LaunchState| {
        let k = s.kind().as_str();
        if states.last() != Some(&k) {
            states.push(k);
        }
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
    let need = Resolution::NeedsProvision(ProvisionNeed::Codename(Channel::UmuProton));
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
    let need = Resolution::NeedsProvision(ProvisionNeed::Codename(Channel::UmuProton));
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

// ---- M7: trusted steps (RUNTIME_MANAGEMENT §4.3) and the audit gate (§6.3) ---------------

use zaun_core::ids::RuntimeName;
use zaun_core::model::{
    Channel, EndReason, NetworkAccess, RuntimeIsolationMode, SessionKind, SlrVariant,
};
use zaun_core::runtime::{AuditAction, AuditDue, TamperSummary};

fn shared(w: &SandboxWorld) -> Resolution {
    let mut rt = w.runtime.clone();
    rt.isolation = RuntimeIsolationMode::Shared;
    Resolution::Ready(rt)
}

fn codename() -> Resolution {
    Resolution::NeedsProvision(ProvisionNeed::Codename(Channel::UmuProton))
}

fn make_prefix(w: &SandboxWorld) {
    w.fs.dir(w.game_paths.prefix.join("drive_c"));
}

/// A hook: the download happened (the resolver now answers `Ready`).
fn becomes(
    res: &Arc<Mutex<Resolution>>,
    r: Resolution,
) -> impl Fn(&launch::LaunchPlan) + Send + Sync + 'static {
    let res = Arc::clone(res);
    move |_| *res.lock() = r.clone()
}

fn env_of_plan(p: &launch::PlannedLaunch, k: &str) -> Option<String> {
    p.sandbox.env.get(k).map(str::to_owned)
}

#[test]
fn ready_with_prefix_goes_straight_to_the_game() {
    let w = world();
    make_prefix(&w);
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    let res = Arc::new(Mutex::new(Resolution::Ready(w.runtime.clone())));
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
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
    assert!(out.trusted.is_empty() && steps.runs().is_empty());
    assert_eq!(steps.launched(), vec![(g.id, w.runtime.name.clone())]);
}

#[test]
fn ready_without_prefix_prepares_online_while_pristine() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    let w2 = Arc::clone(&w);
    steps.push(StepScript::ok().with_hook(move |_| make_prefix(&w2)));
    let res = Arc::new(Mutex::new(Resolution::Ready(w.runtime.clone())));
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
    assert_eq!(
        o.states,
        [
            "validating",
            "resolving_runtime",
            "preparing",
            "resolving_runtime",
            "planning",
            "spawning",
            "starting"
        ]
    );
    let out = o.result.unwrap();
    let runs = steps.runs();
    assert_eq!(runs.len(), 1);
    let prep = &runs[0].plan;
    assert_eq!(prep.session.kind, SessionKind::Prepare);
    assert_eq!(prep.session.game_id, Some(g.id));
    // Pristine prefix + prepare_online: the network is on even for an Offline game.
    assert_eq!(prep.context.network, Some(NetworkAccess::Full));
    assert_eq!(out.trusted.len(), 1);
    assert_eq!(
        steps.installs(),
        1,
        "a network Prepare may change the store"
    );
    assert_eq!(steps.prepared(), vec![(g.id, w.runtime.name.clone())]);
    // The game itself stays offline.
    assert_eq!(l.plans()[0].context.network, Some(NetworkAccess::None));
}

#[test]
fn first_run_from_nothing_prepares_with_the_codename() {
    // §4.3: NeedsProvision, pristine prefix, prepare online → one Prepare that downloads.
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    let res = Arc::new(Mutex::new(codename()));
    let ready = becomes(&res, Resolution::Ready(w.runtime.clone()));
    let w2 = Arc::clone(&w);
    let mut s = StepScript::ok().with_hook(move |p| {
        ready(p);
        make_prefix(&w2);
    });
    s.phases.insert(
        0,
        zaun_core::umu::UmuPhase::DownloadingProton {
            name: "UMU-Proton-10.0-4.tar.gz".into(),
        },
    );
    s.bytes = vec![10 << 20, 20 << 20];
    steps.push(s);
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
    assert_eq!(
        o.states,
        [
            "validating",
            "resolving_runtime",
            "provisioning",
            "preparing",
            "resolving_runtime",
            "planning",
            "spawning",
            "starting"
        ]
    );
    o.result.unwrap();
    let runs = steps.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].plan.session.kind, SessionKind::Prepare);
    assert_eq!(runs[0].plan.context.network, Some(NetworkAccess::Full));
    assert_eq!(
        env_of_plan(&runs[0], "PROTONPATH").as_deref(),
        Some("UMU-Proton")
    );
    assert_eq!(l.plans().len(), 1);
    assert_eq!(l.plans()[0].context.network, Some(NetworkAccess::None));
}

#[test]
fn tainted_prefix_provisions_without_the_game() {
    let w = world();
    make_prefix(&w);
    let mut g = game_of(&w);
    g.prefix_tainted_at = Some(Timestamp::from_millis(1));
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    let res = Arc::new(Mutex::new(codename()));
    steps.push(StepScript::ok().with_hook(becomes(&res, Resolution::Ready(w.runtime.clone()))));
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
    assert_eq!(
        o.states,
        [
            "validating",
            "resolving_runtime",
            "provisioning",
            "resolving_runtime",
            "planning",
            "spawning",
            "starting"
        ]
    );
    let out = o.result.unwrap();
    let p = &steps.runs()[0];
    assert_eq!(p.plan.session.kind, SessionKind::Provision);
    assert_eq!(p.plan.session.game_id, None);
    assert_eq!(p.plan.paths.prefix_lock, None);
    // Never the game's prefix or home (SECURITY_MODEL §4.1 rule 3).
    let prefix = w.game_paths.prefix.to_string_lossy().into_owned();
    assert!(
        p.plan
            .sandbox
            .args
            .iter()
            .all(|a| !a.to_string_lossy().contains(&prefix))
    );
    assert_eq!(steps.installs(), 1);
    assert!(steps.prepared().is_empty());
    assert_eq!(out.trusted[0].kind, SessionKind::Provision);
}

#[test]
fn provision_without_network_is_rt_010() {
    let w = world();
    make_prefix(&w);
    let mut g = game_of(&w);
    g.prefix_tainted_at = Some(Timestamp::from_millis(1));
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    steps.push(StepScript::fails(EndReason::FailedToStart {
        code: ErrorCode::RtNotInstalledOffline,
    }));
    let res = Arc::new(Mutex::new(codename()));
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
    assert_eq!(code(o), ErrorCode::RtNotInstalledOffline);
    assert!(l.plans().is_empty());
}

#[test]
fn prepare_failure_is_reported() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    steps.push(StepScript::fails(EndReason::Exited { code: 1 }));
    let res = Arc::new(Mutex::new(Resolution::Ready(w.runtime.clone())));
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
    assert_eq!(code(o), ErrorCode::RtPrefixPrepareFailed);
    assert!(l.plans().is_empty());
}

#[test]
fn tainted_offline_prefix_never_gets_the_network_without_consent() {
    for consent in [false, true] {
        let w = world();
        let mut g = game_of(&w);
        g.prefix_tainted_at = Some(Timestamp::from_millis(1));
        let l = FakeLauncher::new();
        let steps = ScriptedSteps::new();
        let w2 = Arc::clone(&w);
        steps.push(StepScript::ok().with_hook(move |_| make_prefix(&w2)));
        let res = Arc::new(Mutex::new(Resolution::Ready(w.runtime.clone())));
        run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), consent)
            .result
            .unwrap();
        let prep = &steps.runs()[0].plan;
        assert_eq!(prep.session.kind, SessionKind::Prepare);
        let want = if consent {
            NetworkAccess::Full
        } else {
            NetworkAccess::None
        };
        assert_eq!(prep.context.network, Some(want), "consent {consent}");
        assert_eq!(steps.installs(), usize::from(consent));
    }
}

#[test]
fn shared_mode_waits_for_a_pending_audit() {
    let w = world();
    make_prefix(&w);
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    steps.set_due(AuditDue::Pending);
    let res = Arc::new(Mutex::new(shared(&w)));
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
    assert_eq!(
        o.states,
        [
            "validating",
            "resolving_runtime",
            "auditing_runtime",
            "planning",
            "spawning",
            "starting"
        ]
    );
    assert_eq!(steps.audited(), vec![SlrVariant::Steamrt3]);
    // Pending applies to Shared mode only.
    let steps = ScriptedSteps::new();
    steps.set_due(AuditDue::Pending);
    let res = Arc::new(Mutex::new(Resolution::Ready(w.runtime.clone())));
    let mut iso = w.runtime.clone();
    iso.isolation = RuntimeIsolationMode::Isolated;
    *res.lock() = Resolution::Ready(iso);
    run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false)
        .result
        .unwrap();
    assert!(steps.audited().is_empty());
    // A stale audit applies to every mode.
    steps.set_due(AuditDue::Stale);
    run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false)
        .result
        .unwrap();
    assert_eq!(steps.audited().len(), 1);
}

#[test]
fn tampered_runtime_is_quarantined_and_provisioned_again() {
    // M7 acceptance: a deliberate modification is detected, quarantined and repaired.
    let w = world();
    make_prefix(&w);
    let mut g = game_of(&w);
    g.prefix_tainted_at = Some(Timestamp::from_millis(1));
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    steps.set_due(AuditDue::Pending);
    let res = Arc::new(Mutex::new(shared(&w)));
    let res2 = Arc::clone(&res);
    let quarantined = move || {
        *res2.lock() = Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant: SlrVariant::Steamrt3,
            build: RuntimeName::new("UMU-Proton-10.0-4").unwrap(),
        });
    };
    steps.push_audit(
        AuditAction::Quarantined {
            variant: SlrVariant::Steamrt3,
            quarantine: "/q/steamrt3-1".into(),
            summary: TamperSummary {
                changed: vec!["run".into()],
                changed_count: 1,
                ..TamperSummary::default()
            },
            suspects: vec![zaun_core::runtime::integrity::Suspect {
                game: g.id,
                name: g.name.clone(),
            }],
        },
        Some(Arc::new(quarantined)),
    );
    steps.push(StepScript::ok().with_hook(becomes(&res, shared(&w))));
    let o = run_with(&w, &g, &res, &l, &NoLocks, Some(&steps), false);
    assert_eq!(
        o.states,
        [
            "validating",
            "resolving_runtime",
            "auditing_runtime",
            "resolving_runtime",
            "provisioning",
            "resolving_runtime",
            "planning",
            "spawning",
            "starting"
        ]
    );
    let out = o.result.unwrap();
    assert_eq!(out.notices.len(), 1);
    assert_eq!(out.notices[0].code, ErrorCode::RtSharedTampered);
    assert_eq!(steps.runs()[0].plan.session.kind, SessionKind::Provision);
    // The fresh install is not audited again in the same launch.
    assert_eq!(steps.audited().len(), 1);
}

#[test]
fn dry_run_has_no_side_effects() {
    let w = world();
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let steps = ScriptedSteps::new();
    let clock = FakeClock::baseline();
    let before = w.fs.paths();
    let writes_before = w.fs.total_calls();
    for (res, want) in [
        (codename(), DryStep::Prepare),
        (Resolution::Ready(w.runtime.clone()), DryStep::Prepare),
    ] {
        let runtimes = Scripted {
            resolution: Arc::new(Mutex::new(res)),
            pool: JobPool::new(JobPoolConfig::default(), Arc::new(FakeClock::baseline())).unwrap(),
        };
        let env = env_of(&w);
        let deps = OrchestratorDeps {
            host: &w.host,
            env: &env,
            runtimes: &runtimes,
            launcher: &l,
            locks: &NoLocks,
            fs: Arc::new(WorldFs(Arc::clone(&w))),
            clock: &clock,
            steps: Some(&steps),
            cancel: None,
        };
        let req = LaunchRequest {
            game: &g,
            exe: None,
            extra_args: &[],
            session_id: SessionId::generate(&clock as &dyn Clock),
            network_consent: false,
        };
        let d = launch::dry_run(&req, &deps).unwrap();
        assert_eq!(d.step, want);
        assert!(d.note.is_some());
        assert_eq!(d.planned.plan.session.kind, SessionKind::Prepare);
    }
    assert_eq!(w.fs.paths(), before);
    assert!(w.fs.total_calls() >= writes_before);
    assert!(l.plans().is_empty() && steps.runs().is_empty());
    assert!(steps.prepared().is_empty() && steps.launched().is_empty());
}

/// DEVELOPER_MODE §9 "inspector equals execution" (DEV-001): for the same session, the
/// dry run's argument vector, descriptors and command (what the inspector's *Raw* tab shows)
/// equal what the spawner hand-off receives.
#[test]
fn inspector_equals_execution() {
    let w = world();
    make_prefix(&w);
    let g = game_of(&w);
    let l = FakeLauncher::new();
    let clock = FakeClock::baseline();
    let runtimes = Scripted {
        resolution: Arc::new(Mutex::new(Resolution::Ready(w.runtime.clone()))),
        pool: JobPool::new(JobPoolConfig::default(), Arc::new(FakeClock::baseline())).unwrap(),
    };
    let env = env_of(&w);
    let deps = OrchestratorDeps {
        host: &w.host,
        env: &env,
        runtimes: &runtimes,
        launcher: &l,
        locks: &NoLocks,
        fs: Arc::new(WorldFs(Arc::clone(&w))),
        clock: &clock,
        steps: None,
        cancel: None,
    };
    let req = LaunchRequest {
        game: &g,
        exe: None,
        extra_args: &[],
        session_id: SessionId::generate(&clock as &dyn Clock),
        network_consent: false,
    };
    let dry = launch::dry_run(&req, &deps).unwrap();
    assert_eq!(dry.step, DryStep::Game);
    launch::launch(&req, &deps, &mut |_: &LaunchState| {}).unwrap();
    let executed = l.plans().pop().expect("handed off");
    assert_eq!(dry.planned.plan.sandbox, executed.sandbox);
    assert_eq!(dry.planned.plan.bwrap, executed.bwrap);
    assert_eq!(dry.planned.plan.host_actions, executed.host_actions);
    let data = launch::InspectorData {
        source: launch::InspectorSource::DryRun,
        plan: dry.planned.plan.clone(),
        sandbox: Some(dry.planned.sandbox.clone()),
        explain: Vec::new(),
        timeline: Vec::new(),
        session: None,
        notes: Vec::new(),
    };
    let raw: Vec<String> = launch::inspect::raw_args(&data.plan)
        .into_iter()
        .map(|(_, a)| a)
        .collect();
    let spawned: Vec<String> = executed
        .sandbox
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(raw, spawned);
    // Every rendered mount appears in the table with its section and reason.
    let rows = launch::inspect::mount_rows(&data);
    assert_eq!(rows.len(), dry.planned.sandbox.mounts.len());
    assert!(
        rows.iter()
            .all(|r| r.section.is_some() && r.reason.is_some())
    );
}
