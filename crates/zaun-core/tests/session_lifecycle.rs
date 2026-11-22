//! Process lifecycle tests (TESTING_STRATEGY §6; PROC-001…PROC-013, INIT-001 end to end).
//!
//! The supervisor runs in-process (on a thread, stop requests through its `StopTrigger`)
//! against `fake-bwrap` and `fake-umu-run` with the real `zaun-init`, so the suite needs
//! no user namespaces. The launcher tests start the real `zaun supervise` detached.
//! Tests marked `e2e_` repeat the namespace-dependent rows under real `bwrap` and run only
//! with `ZAUN_E2E=1`. Within a test process the tests run one at a time; the ones that
//! start helper processes, which are counted by name in the whole process table, also take
//! turns across test processes (`helpers_turn`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use zaun_core::clock::{Clock, SystemClock, Timestamp};
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::ids::{GameId, SessionId};
use zaun_core::launch::{LAUNCH_PLAN_FORMAT, LaunchPlan, PlanPaths, PlanSandbox, PlanSession};
use zaun_core::model::{
    EndReason, Session, SessionContext, SessionKind, SessionState, SlrVariant, SupervisorIdent,
};
use zaun_core::paths::AppPaths;
use zaun_core::procfs::{ProcSource, RealProc};
use zaun_core::session::supervisor::{self, FinalRecord};
use zaun_core::session::sys::Sig;
use zaun_core::session::{
    PrefixLock, RealLauncher, RealSpawner, RecoveryOptions, SessionLauncher, SessionStore,
    StopMode, StopOutcome, StopTrigger, SupervisorConfig, SupervisorDeps, SupervisorOutcome,
    recover,
};
use zaun_core::stats::CountRule;
use zaun_core::storage::{
    CloseOutcome, Db, GameRepo, MigrateOptions, SessionClose, SessionRepo, StatsRepo, StorageError,
    StorageResult,
};
use zaun_testkit::bins::workspace_bin;
use zaun_testkit::fixtures::games;
use zaun_testkit::{FakeProc, TestEnv, proc_entry};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// The turn of a test that starts `fake-child` or `fake-detached` processes. They are
/// counted by name in the whole process table and nextest runs every test in a process of
/// its own, so the in-process lock does not keep such tests apart: a lock file does.
fn helpers_turn() -> std::fs::File {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("session-helpers.lock");
    let file = std::fs::File::create(path).unwrap();
    file.lock().unwrap();
    file
}

fn e2e() -> bool {
    std::env::var_os("ZAUN_E2E").is_some_and(|v| v == "1")
}

/// A temporary installation with a migrated library holding one game.
struct World {
    env: TestEnv,
    paths: AppPaths,
    game: GameId,
}

impl World {
    fn new() -> Self {
        let env = TestEnv::new().unwrap();
        let paths = env.paths();
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let mut db = Db::open_rw(&paths.db_file).unwrap();
        db.migrate(&MigrateOptions::default()).unwrap();
        let g = games::game(1, 1);
        db.insert_game(&g).unwrap();
        Self {
            env,
            paths,
            game: g.id,
        }
    }

    fn db(&self) -> Db {
        Db::open_rw(&self.paths.db_file).unwrap()
    }

    fn rows(&self) -> Vec<Session> {
        self.db().session_history(None, &[], 0, 100).unwrap()
    }
}

struct PlanOpts<'a> {
    fake: &'a str,
    exe: &'a str,
    grace: u8,
    kind: SessionKind,
    real_bwrap: bool,
}

impl Default for PlanOpts<'_> {
    fn default() -> Self {
        Self {
            fake: "exit:0",
            exe: "Game.exe",
            grace: 10,
            kind: SessionKind::Game,
            real_bwrap: false,
        }
    }
}

fn os(v: &[&str]) -> Vec<OsString> {
    v.iter().map(OsString::from).collect()
}

fn find_bwrap() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    ["/usr/bin", "/bin", "/run/current-system/sw/bin"]
        .iter()
        .map(PathBuf::from)
        .chain(std::env::split_paths(&path))
        .map(|d| d.join("bwrap"))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("bwrap"))
}

/// A plan running `zaun-init -- fake-umu-run C:/games/<exe>` with `FAKE_UMU=<fake>`.
fn plan(w: &World, o: &PlanOpts<'_>) -> LaunchPlan {
    let sid = SessionId::generate(&SystemClock::new());
    let init = workspace_bin("zaun-init");
    let umu = workspace_bin("fake-umu-run");
    let mut args = os(&[
        "--unshare-pid",
        "--die-with-parent",
        "--new-session",
        "--json-status-fd",
        "4",
        "--sync-fd",
        "5",
    ]);
    if o.real_bwrap {
        args = os(&[
            "--unshare-user",
            "--unshare-pid",
            "--die-with-parent",
            "--new-session",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/run",
            "--json-status-fd",
            "4",
            "--sync-fd",
            "5",
        ]);
    }
    args.extend([
        "--ro-bind".into(),
        init.into_os_string(),
        "/run/zaun/init".into(),
        "--setenv".into(),
        "FAKE_UMU".into(),
        o.fake.into(),
    ]);
    LaunchPlan {
        format: LAUNCH_PLAN_FORMAT,
        session: PlanSession {
            id: sid,
            kind: o.kind,
            game_id: Some(w.game),
            game_name: "Test Game".into(),
            executable_id: None,
            counted_eligible: o.kind == SessionKind::Game,
            stop_grace_secs: o.grace,
        },
        context: SessionContext::default(),
        bwrap: if o.real_bwrap {
            find_bwrap()
        } else {
            workspace_bin("fake-bwrap")
        },
        sandbox: PlanSandbox {
            args,
            fds: Vec::new(),
            command: vec![
                "/run/zaun/init".into(),
                "--status-fd".into(),
                "6".into(),
                "--".into(),
                umu.into_os_string(),
                format!("C:/games/{}", o.exe).into(),
            ],
        },
        host_actions: Vec::new(),
        paths: PlanPaths {
            log_dir: w.paths.session_dir(sid),
            runtime_dir: w.paths.run_session_dir(sid),
            session_var: None,
            prefix_lock: Some(w.paths.prefix_lock(w.game)),
        },
        audit_after: None,
    }
}

fn cfg(w: &World) -> SupervisorConfig {
    let mut c = SupervisorConfig::new(w.env.uid());
    c.close_retry_delay = Duration::from_millis(10);
    c
}

fn deps_with(
    store: Box<dyn SessionStore>,
    clock: Arc<dyn Clock>,
    trigger: &StopTrigger,
) -> SupervisorDeps {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    SupervisorDeps {
        store,
        clock,
        proc: Arc::new(RealProc::new(Arc::clone(&fs))),
        fs,
        spawner: Box::new(RealSpawner),
        trigger: trigger.clone(),
    }
}

/// A supervisor running on a thread.
struct Running {
    handle: std::thread::JoinHandle<SupervisorOutcome>,
    trigger: StopTrigger,
    plan: LaunchPlan,
    started: Instant,
}

fn start_with(
    w: &World,
    plan: LaunchPlan,
    cfg: SupervisorConfig,
    store: Box<dyn SessionStore>,
    clock: Arc<dyn Clock>,
) -> Running {
    let fs = RealFs::new();
    fs.create_dir_all(&plan.paths.runtime_dir, 0o700).unwrap();
    fs.create_dir_all(&plan.paths.log_dir, 0o700).unwrap();
    let _ = w;
    let trigger = StopTrigger::new().unwrap();
    let deps = deps_with(store, clock, &trigger);
    let p = plan.clone();
    let handle = std::thread::spawn(move || supervisor::run(&p, &cfg, deps));
    Running {
        handle,
        trigger,
        plan,
        started: Instant::now(),
    }
}

fn start(w: &World, o: &PlanOpts<'_>) -> Running {
    start_with(
        w,
        plan(w, o),
        cfg(w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    )
}

impl Running {
    fn id(&self) -> SessionId {
        self.plan.session.id
    }

    /// Ask for a stop the way `session::stop::request_stop` does (request file + signal).
    fn stop(&self, mode: StopMode) {
        std::fs::write(
            self.plan.paths.runtime_dir.join("stop-request"),
            format!("{}\n", mode.as_str()),
        )
        .unwrap();
        self.trigger.raise(match mode {
            StopMode::Graceful => Sig::Term,
            StopMode::Force => Sig::Usr1,
        });
    }

    fn join(self) -> (SupervisorOutcome, Duration) {
        let o = self.handle.join().unwrap();
        (o, self.started.elapsed())
    }
}

fn wait_state(w: &World, id: SessionId, want: SessionState, timeout: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < timeout {
        if let Ok(Some(s)) = w.db().session(id)
            && s.state == want
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn ended(w: &World, id: SessionId) -> Session {
    let s = w.db().session(id).unwrap().unwrap();
    assert_eq!(s.state, SessionState::Ended, "{s:?}");
    s
}

/// Processes with one of these `comm`s (helpers of `fake-umu-run`).
fn helpers_alive(comms: &[&str]) -> Vec<u32> {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let proc = RealProc::new(fs);
    proc.snapshot()
        .unwrap()
        .into_iter()
        .filter(|e| comms.contains(&e.comm.as_str()) && !e.state.is_gone())
        .map(|e| e.pid)
        .collect()
}

// ---- scenarios ------------------------------------------------------------------------------

#[test]
fn exits_0_after_2_s() {
    let _s = serial();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:2000,exit:0",
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    let (out, wall) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Exited { code: 0 },
                counted: false,
                recorded: true,
                ..
            }
        ),
        "{out:?}"
    );
    let s = ended(&w, id);
    let d = s.duration_ms.unwrap();
    assert!(
        (2000..wall.as_millis() as u64 + 50).contains(&d),
        "{d} vs {wall:?}"
    );
    assert!(!s.counted, "shorter than the minimum");
    assert_eq!(w.rows().len(), 1, "exactly one row");
    let events = w.db().session_events(id).unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert!(kinds.contains(&"phase"), "{kinds:?}");
    assert!(
        events
            .iter()
            .any(|e| e.kind == "state" && e.data_json.contains("running")),
        "Running reached through the target comm"
    );
    let log = std::fs::read_to_string(w.paths.session_dir(id).join("game.log")).unwrap();
    assert!(log.contains("umu-launcher version"), "{log}");
    assert!(
        !w.paths.run_session_dir(id).exists(),
        "runtime dir cleaned up"
    );
}

#[test]
fn counted_session_updates_statistics() {
    let _s = serial();
    let w = World::new();
    let mut c = cfg(&w);
    c.count_rule = CountRule::new(true, 1);
    let p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:1500,exit:0",
            ..PlanOpts::default()
        },
    );
    let r = start_with(&w, p, c, Box::new(w.db()), Arc::new(SystemClock::new()));
    let id = r.id();
    let (out, _) = r.join();
    assert!(
        matches!(out, SupervisorOutcome::Ended { counted: true, .. }),
        "{out:?}"
    );
    let s = ended(&w, id);
    let stats = w.db().game_stats(w.game).unwrap().unwrap();
    assert_eq!(stats.sessions, 1);
    assert_eq!(stats.total_ms, s.duration_ms.unwrap());
}

#[test]
fn umu_traceback_is_failed_to_start() {
    let _s = serial();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,traceback,exit:1",
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::FailedToStart {
                    code: ErrorCode::UmuInternalError
                },
                counted: false,
                ..
            }
        ),
        "{out:?}"
    );
    let s = ended(&w, id);
    assert_eq!(s.exit_status, Some(1));
    let stats = w.db().game_stats(w.game).unwrap().unwrap();
    assert_eq!(stats.failed_launches, 1);
    assert_eq!(stats.sessions, 0);
}

/// The session's sound server (ADR-0026) runs before the sandbox, from a configuration with a
/// secret and only the three variables it needs, and is gone when the session ends.
#[test]
fn sound_server_runs_for_the_session_and_stops_with_it() {
    use zaun_core::sandbox::HostAction;
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let mut p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:300,exit:0",
            ..PlanOpts::default()
        },
    );
    let dir = p.paths.runtime_dir.join("sound");
    let log = p.paths.log_dir.join("sound-server.log");
    p.host_actions.push(HostAction::StartSoundServer {
        program: workspace_bin("fake-pipewire"),
        dir: dir.clone(),
        remote: "/run/user/1000/pipewire-0".into(),
        log: log.clone(),
    });
    let r = start_with(
        &w,
        p,
        cfg(&w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    );
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Exited { code: 0 },
                ..
            }
        ),
        "{out:?}"
    );
    let text = String::from_utf8(RealFs::new().read(&log, 1 << 16).unwrap()).unwrap();
    assert!(text.contains("listening on "), "{text}");
    let env = text
        .lines()
        .find_map(|l| l.strip_prefix("env: "))
        .unwrap_or_default();
    let mut expected = vec!["PIPEWIRE_CONFIG_DIR", "PIPEWIRE_REMOTE"];
    if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
        expected.push("XDG_RUNTIME_DIR");
    }
    assert_eq!(env.split(' ').collect::<Vec<_>>(), expected);
    assert!(text.contains("secret: 32 hex digits"), "{text}");
    assert!(text.contains("config mode: 600"), "{text}");
    assert!(helpers_alive(&["fake-pipewire"]).is_empty());
    assert!(!dir.exists());
}

#[test]
fn a_sound_server_that_does_not_start_fails_the_launch() {
    use zaun_core::sandbox::HostAction;
    let _s = serial();
    let w = World::new();
    let mut p = plan(&w, &PlanOpts::default());
    p.host_actions.push(HostAction::StartSoundServer {
        program: "/nonexistent/pipewire".into(),
        dir: p.paths.runtime_dir.join("sound"),
        remote: "/run/user/1000/pipewire-0".into(),
        log: p.paths.log_dir.join("sound-server.log"),
    });
    let r = start_with(
        &w,
        p,
        cfg(&w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    );
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                spawn_failed: true,
                ..
            }
        ),
        "{out:?}"
    );
}

/// A restricted Wayland socket that cannot be created (ADR-0029) fails the launch before the
/// sandbox starts (the fake `bwrap` would otherwise end the session with status 0), and the
/// sound server already started for the session is stopped. The sound server is left out
/// where its socket path would not fit a socket address (a long `TMPDIR`): it would fail
/// first and the test would prove nothing about the context.
#[test]
fn a_wayland_context_that_cannot_be_created_fails_the_launch() {
    use zaun_core::sandbox::HostAction;
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let mut p = plan(&w, &PlanOpts::default());
    let sound = p.paths.runtime_dir.join("sound");
    let with_sound = sound.join("native").as_os_str().len() <= 107;
    if with_sound {
        p.host_actions.push(HostAction::StartSoundServer {
            program: workspace_bin("fake-pipewire"),
            dir: sound.clone(),
            remote: "/run/user/1000/pipewire-0".into(),
            log: p.paths.log_dir.join("sound-server.log"),
        });
    } else {
        eprintln!("without the sound server: {} is too long", sound.display());
    }
    p.host_actions.push(HostAction::CreateWaylandContext {
        display: "/nonexistent/wayland-0".into(),
        socket: p.paths.runtime_dir.join("wayland"),
        app_id: "test".into(),
        instance_id: "1".into(),
    });
    let r = start_with(
        &w,
        p,
        cfg(&w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    );
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::FailedToStart {
                    code: ErrorCode::SbxSetupFailed
                },
                spawn_failed: true,
                ..
            }
        ),
        "{out:?}"
    );
    assert!(helpers_alive(&["fake-pipewire"]).is_empty());
    assert!(!sound.exists());
}

#[test]
fn detached_child_keeps_the_session_open() {
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:700,detach:2500,exit:0",
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    assert!(
        wait_state(&w, id, SessionState::Lingering, Duration::from_secs(3)),
        "Lingering after the main program exited"
    );
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Exited { code: 0 },
                ..
            }
        ),
        "{out:?}"
    );
    let s = ended(&w, id);
    assert!(s.duration_ms.unwrap() >= 3100, "{:?}", s.duration_ms);
    assert_eq!(w.rows().len(), 1);
    assert!(helpers_alive(&["fake-detached"]).is_empty());
}

#[test]
fn force_stop_leaves_no_process_behind() {
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,children:10:60000,game,sleep:60000",
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    assert!(wait_state(
        &w,
        id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(helpers_alive(&["fake-child", "fake-detached"]).len(), 10);
    r.stop(StopMode::Force);
    let (out, wall) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Killed,
                ..
            }
        ),
        "{out:?}"
    );
    assert!(wall < Duration::from_secs(10));
    std::thread::sleep(Duration::from_millis(200));
    assert!(helpers_alive(&["fake-child", "fake-detached"]).is_empty());
    assert_eq!(ended(&w, id).end_reason, Some(EndReason::Killed));
}

#[test]
fn graceful_stop_of_a_cooperative_program() {
    let _s = serial();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:60000",
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    assert!(wait_state(
        &w,
        id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    let t = Instant::now();
    r.stop(StopMode::Graceful);
    let (out, _) = r.join();
    assert!(t.elapsed() < Duration::from_secs(3), "SIGTERM was enough");
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::StoppedByUser,
                ..
            }
        ),
        "{out:?}"
    );
    let s = ended(&w, id);
    assert_eq!(s.exit_status, Some(143));
    let events = w.db().session_events(id).unwrap();
    assert!(events.iter().any(|e| e.kind == "stop_requested"));
    assert!(
        events
            .iter()
            .any(|e| e.kind == "state" && e.data_json.contains("stopping"))
    );
}

#[test]
fn ignoring_sigterm_is_killed_after_the_grace_period() {
    let _s = serial();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,game,ignore-term,sleep:60000",
            grace: 1,
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    assert!(wait_state(
        &w,
        id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    let t = Instant::now();
    r.stop(StopMode::Graceful);
    let (out, _) = r.join();
    let took = t.elapsed();
    assert!(took >= Duration::from_secs(1), "{took:?}");
    assert!(
        took < Duration::from_secs(1 + 4 + 1),
        "gone within grace + 4 s: {took:?}"
    );
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Killed,
                ..
            }
        ),
        "{out:?}"
    );
}

#[test]
fn sigsegv_is_a_crash() {
    let _s = serial();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:800,segv",
            ..PlanOpts::default()
        },
    );
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Crashed { signal: 11 },
                ..
            }
        ),
        "{out:?}"
    );
}

#[test]
fn large_output_is_capped_and_never_blocks() {
    let _s = serial();
    let w = World::new();
    // 64× the cap is plenty to show it never blocks; the e2e run drains a full GiB.
    let mib = if e2e() { 1024 } else { 64 };
    let mut c = cfg(&w);
    c.log_cap_bytes = 1 << 20;
    let p = plan(
        &w,
        &PlanOpts {
            fake: &format!("spam:{mib},exit:0"),
            ..PlanOpts::default()
        },
    );
    let r = start_with(&w, p, c, Box::new(w.db()), Arc::new(SystemClock::new()));
    let id = r.id();
    let (out, wall) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Exited { code: 0 },
                ..
            }
        ),
        "{out:?}"
    );
    let log = std::fs::metadata(w.paths.session_dir(id).join("game.log")).unwrap();
    assert!(log.len() <= (1 << 20) + 4096, "{}", log.len());
    eprintln!("{mib} MiB drained in {wall:?}");
}

#[test]
fn second_launch_of_the_same_game_is_refused() {
    let _s = serial();
    let w = World::new();
    let _held = PrefixLock::try_acquire(&w.paths.prefix_lock(w.game)).unwrap();
    let p = plan(&w, &PlanOpts::default());
    let run_dir = p.paths.runtime_dir.clone();
    let r = start_with(
        &w,
        p,
        cfg(&w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    );
    let (out, _) = r.join();
    assert!(matches!(out, SupervisorOutcome::LockHeld));
    assert_eq!(out.exit_code(), 3);
    assert!(w.rows().is_empty(), "no row");
    assert_eq!(
        std::fs::read_to_string(run_dir.join("start-failed"))
            .unwrap()
            .trim(),
        "ZN-SES-010"
    );
}

#[test]
fn stop_during_starting() {
    let _s = serial();
    let w = World::new();
    // No `game` step: nothing looks like the game, the session stays Starting.
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,sleep:60000",
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(
        w.db().session(id).unwrap().unwrap().state,
        SessionState::Starting
    );
    r.stop(StopMode::Graceful);
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::StoppedByUser,
                counted: false,
                ..
            }
        ),
        "{out:?}"
    );
}

#[test]
fn spawn_failure_is_recorded() {
    let _s = serial();
    let w = World::new();
    let mut p = plan(&w, &PlanOpts::default());
    p.bwrap = PathBuf::from("/nonexistent/bwrap");
    let r = start_with(
        &w,
        p,
        cfg(&w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    );
    let id = r.id();
    let (out, _) = r.join();
    assert_eq!(out.exit_code(), 4);
    assert_eq!(
        ended(&w, id).end_reason,
        Some(EndReason::FailedToStart {
            code: ErrorCode::HostBwrapNotFound
        })
    );
}

#[test]
fn bad_bwrap_options_fail_to_start() {
    let _s = serial();
    let w = World::new();
    let mut p = plan(&w, &PlanOpts::default());
    p.sandbox.args.insert(0, "--perms".into());
    p.sandbox.args.insert(1, "0700".into());
    let r = start_with(
        &w,
        p,
        cfg(&w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    );
    let id = r.id();
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::FailedToStart {
                    code: ErrorCode::SbxSetupFailed
                },
                ..
            }
        ),
        "{out:?}"
    );
    let log = std::fs::read_to_string(w.paths.session_dir(id).join("game.log")).unwrap();
    assert!(log.contains("bwrap: --perms"), "{log}");
}

#[test]
fn clock_jump_and_suspend_follow_the_monotonic_clock() {
    let _s = serial();
    let w = World::new();
    let clock = Arc::new(zaun_testkit::FakeClock::baseline());
    let c2 = Arc::clone(&clock);
    let p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:1500,exit:0",
            ..PlanOpts::default()
        },
    );
    let r = start_with(&w, p, cfg(&w), Box::new(w.db()), clock);
    let id = r.id();
    let jumper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        // Ten minutes of play, the wall clock thrown back a day (NTP), then a suspend of an
        // hour (wall only).
        c2.advance_mono(Duration::from_secs(600));
        c2.set_wall(Timestamp::from_millis(c2.now().as_millis() - 86_400_000));
        c2.advance_wall(Duration::from_secs(3600));
    });
    let (out, _) = r.join();
    jumper.join().unwrap();
    assert!(
        matches!(out, SupervisorOutcome::Ended { counted: true, .. }),
        "{out:?}"
    );
    let s = ended(&w, id);
    assert_eq!(s.duration_ms, Some(600_000));
    assert!(
        s.ended_at.unwrap() < s.started_at,
        "wall time stored as observed"
    );
    assert_eq!(
        w.db().game_stats(w.game).unwrap().unwrap().total_ms,
        600_000
    );
}

/// A store whose closing transaction always fails (database locked at the end).
struct FailingClose(Db);

impl SessionStore for FailingClose {
    fn insert(&mut self, s: &Session) -> StorageResult<()> {
        self.0.insert(s)
    }
    fn set_state(&mut self, id: SessionId, st: SessionState) -> StorageResult<bool> {
        self.0.set_state(id, st)
    }
    fn heartbeat(&mut self, id: SessionId, ms: u64, at: Timestamp) -> StorageResult<bool> {
        SessionStore::heartbeat(&mut self.0, id, ms, at)
    }
    fn set_sandbox(
        &mut self,
        id: SessionId,
        p: Option<u32>,
        n: Option<u64>,
    ) -> StorageResult<bool> {
        self.0.set_sandbox(id, p, n)
    }
    fn event(&mut self, id: SessionId, at: Timestamp, k: &str, d: &str) -> StorageResult<()> {
        self.0.event(id, at, k, d)
    }
    fn mark_tainted(&mut self, g: GameId, at: Timestamp) -> StorageResult<()> {
        self.0.mark_tainted(g, at)
    }
    fn close(&mut self, _: &SessionClose, _: Option<SlrVariant>) -> StorageResult<CloseOutcome> {
        Err(StorageError::Busy)
    }
}

#[test]
fn database_locked_at_the_end_writes_final_json() {
    let _s = serial();
    let w = World::new();
    let mut c = cfg(&w);
    c.count_rule = CountRule::new(true, 0);
    let p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:600,exit:0",
            ..PlanOpts::default()
        },
    );
    let r = start_with(
        &w,
        p,
        c,
        Box::new(FailingClose(w.db())),
        Arc::new(SystemClock::new()),
    );
    let id = r.id();
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                recorded: false,
                ..
            }
        ),
        "{out:?}"
    );
    let final_path = w.paths.session_dir(id).join("final.json");
    let rec: FinalRecord = serde_json::from_slice(&std::fs::read(&final_path).unwrap()).unwrap();
    assert_eq!(rec.session, id);
    assert!(w.db().session(id).unwrap().unwrap().is_open());

    // Recovery imports it; a second pass changes nothing. (The in-process supervisor's
    // identity is this test process, so recovery sees a process table without it.)
    let fs = RealFs::new();
    let real = RealProc::new(Arc::new(RealFs::new()));
    let proc = FakeProc::new();
    proc.set_boot_id(&real.boot_id().unwrap());
    let opts = RecoveryOptions {
        count_rule: CountRule::new(true, 0),
        ..RecoveryOptions::default()
    };
    let mut db = w.db();
    let rec1 = recover(&mut db, &proc, &fs, &w.paths, &SystemClock::new(), &opts).unwrap();
    assert_eq!(rec1.len(), 1);
    assert!(rec1[0].from_final);
    let rec2 = recover(&mut db, &proc, &fs, &w.paths, &SystemClock::new(), &opts).unwrap();
    assert!(rec2.is_empty());
    let s = ended(&w, id);
    assert_eq!(s.end_reason, Some(EndReason::Exited { code: 0 }));
    assert_eq!(s.duration_ms, Some(rec.duration_ms));
    let stats = db.game_stats(w.game).unwrap().unwrap();
    assert_eq!(stats.sessions, 1, "aggregates applied once");
}

// ---- recovery with fakes ----------------------------------------------------------------------

fn open_row(w: &World, k: u32, pid: u32, ticks: u64, boot: &str, active_ms: u64) -> SessionId {
    let g = w.db().game(w.game).unwrap().unwrap();
    let mut s = games::session(&g, 1, k, Timestamp::from_millis(games::BASE_MS));
    s.state = SessionState::Running;
    s.active_ms = active_ms;
    s.last_heartbeat_at = Some(Timestamp::from_millis(games::BASE_MS + active_ms as i64));
    s.supervisor = Some(SupervisorIdent {
        pid,
        start_ticks: ticks,
        boot_id: boot.into(),
    });
    w.db().insert_session(&s).unwrap();
    s.id
}

#[test]
fn recovery_branches_with_fake_processes() {
    let _s = serial();
    let w = World::new();
    let proc = FakeProc::new();
    proc.set_boot_id("boot-now");
    proc.insert(proc_entry(100, 1, "zaun", 0));
    proc.insert(proc_entry(200, 1, "something-else", 0));
    // proc_entry start ticks: compare with what the fixture uses.
    let alive_ticks = proc.entry(100).unwrap().unwrap().start_time_ticks;
    let reused_ticks = proc.entry(200).unwrap().unwrap().start_time_ticks;

    let alive = open_row(&w, 1, 100, alive_ticks, "boot-now", 40_000);
    let reused = open_row(&w, 2, 200, reused_ticks + 1, "boot-now", 50_000);
    let dead = open_row(&w, 3, 300, 7, "boot-now", 60_000);
    let rebooted = open_row(&w, 4, 100, alive_ticks, "boot-old", 70_000);

    let fs = RealFs::new();
    let mut db = w.db();
    let out = recover(
        &mut db,
        &proc,
        &fs,
        &w.paths,
        &SystemClock::new(),
        &RecoveryOptions::default(),
    )
    .unwrap();
    let by = |id| {
        out.iter()
            .find(|r| r.id == id)
            .map(|r| (r.reason, r.duration_ms))
    };
    assert_eq!(by(alive), None, "a live supervisor is kept");
    assert_eq!(
        by(reused),
        Some((EndReason::LauncherTerminated, 50_000)),
        "PID reuse is not mistaken for the supervisor"
    );
    assert_eq!(by(dead), Some((EndReason::LauncherTerminated, 60_000)));
    assert_eq!(by(rebooted), Some((EndReason::Interrupted, 70_000)));
    let s = db.session(dead).unwrap().unwrap();
    assert_eq!(s.ended_at, s.last_heartbeat_at);
    assert!(s.counted);
    assert!(db.session(alive).unwrap().unwrap().is_open());
    // Idempotent.
    assert!(
        recover(
            &mut db,
            &proc,
            &fs,
            &w.paths,
            &SystemClock::new(),
            &RecoveryOptions::default()
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(db.game_stats(w.game).unwrap().unwrap().sessions, 3);
}

// ---- the detached supervisor ------------------------------------------------------------------

fn launcher(w: &World) -> RealLauncher {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let mut l = RealLauncher::new(
        workspace_bin("zaun"),
        w.paths.clone(),
        Arc::clone(&fs),
        Arc::new(RealProc::new(fs)),
        Arc::new(SystemClock::new()),
    );
    l.env = w
        .env
        .envs()
        .into_iter()
        .chain([("PATH".into(), std::env::var_os("PATH").unwrap_or_default())])
        .collect();
    l
}

fn wait_ended(w: &World, id: SessionId, timeout: Duration) -> Session {
    let t = Instant::now();
    loop {
        let s = w.db().session(id).unwrap().unwrap();
        if !s.is_open() {
            return s;
        }
        assert!(t.elapsed() < timeout, "session did not end: {s:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn launcher_starts_a_detached_supervisor_and_stops_it() {
    let _s = serial();
    let w = World::new();
    let l = launcher(&w);
    let p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:60000",
            ..PlanOpts::default()
        },
    );
    let h = l.start(&p).unwrap();
    assert_eq!(h.session_id, p.session.id);
    let proc = RealProc::new(Arc::new(RealFs::new()));
    let sup = proc.entry(h.supervisor.pid).unwrap().unwrap();
    assert_ne!(sup.ppid, std::process::id(), "not a child of the launcher");
    assert!(w.paths.session_dir(h.session_id).join("plan.txt").is_file());
    assert!(wait_state(
        &w,
        h.session_id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    let row = w.db().session(h.session_id).unwrap().unwrap();
    assert_eq!(
        l.request_stop(&row, StopMode::Graceful).unwrap(),
        StopOutcome::Signalled
    );
    let s = wait_ended(&w, h.session_id, Duration::from_secs(10));
    assert_eq!(s.end_reason, Some(EndReason::StoppedByUser));
    let log =
        std::fs::read_to_string(w.paths.session_dir(h.session_id).join("supervisor.log")).unwrap();
    assert!(log.contains("session ended"), "{log}");
    // The supervisor records the end, then exits: wait for the exit.
    let t = std::time::Instant::now();
    while proc.entry(h.supervisor.pid).unwrap().is_some() {
        assert!(t.elapsed() < Duration::from_secs(5), "the supervisor did not exit");
        std::thread::sleep(Duration::from_millis(50));
    }
    // Stopping again is idempotent.
    assert_eq!(
        l.request_stop(&s, StopMode::Force).unwrap(),
        StopOutcome::SupervisorGone
    );
}

#[test]
fn launcher_force_stop() {
    let _s = serial();
    let w = World::new();
    let l = launcher(&w);
    let p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,ignore-term,sleep:60000",
            ..PlanOpts::default()
        },
    );
    let h = l.start(&p).unwrap();
    assert!(wait_state(
        &w,
        h.session_id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    let row = w.db().session(h.session_id).unwrap().unwrap();
    l.request_stop(&row, StopMode::Force).unwrap();
    let s = wait_ended(&w, h.session_id, Duration::from_secs(6));
    assert_eq!(s.end_reason, Some(EndReason::Killed));
}

#[test]
fn launcher_reports_a_held_lock() {
    let _s = serial();
    let w = World::new();
    let _held = PrefixLock::try_acquire(&w.paths.prefix_lock(w.game)).unwrap();
    let l = launcher(&w);
    let err = l.start(&plan(&w, &PlanOpts::default())).unwrap_err();
    assert_eq!(err.code, ErrorCode::SesAlreadyRunning);
    assert!(w.rows().is_empty());
}

#[test]
fn killed_supervisor_is_recovered() {
    let _s = serial();
    let w = World::new();
    let l = launcher(&w);
    let p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:4000",
            ..PlanOpts::default()
        },
    );
    let h = l.start(&p).unwrap();
    assert!(wait_state(
        &w,
        h.session_id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    let proc = RealProc::new(Arc::new(RealFs::new()));
    zaun_core::session::sys::signal_identity(&proc, h.supervisor, Sig::Kill).unwrap();
    let t = Instant::now();
    while proc.is_alive(h.supervisor).unwrap() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut db = w.db();
    let out = recover(
        &mut db,
        &proc,
        &RealFs::new(),
        &w.paths,
        &SystemClock::new(),
        &RecoveryOptions::default(),
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].reason, EndReason::LauncherTerminated);
    let s = ended(&w, h.session_id);
    assert_eq!(s.duration_ms, Some(s.active_ms));
    assert_eq!(w.rows().len(), 1);
}

// ---- real bwrap (ZAUN_E2E=1) ------------------------------------------------------------

fn namespace_members(ns: u64) -> usize {
    let proc = RealProc::new(Arc::new(RealFs::new()));
    proc.snapshot()
        .unwrap()
        .into_iter()
        .filter(|e| e.pid_ns == Some(ns) && !e.state.is_gone())
        .count()
}

#[test]
fn e2e_detached_child_under_real_bwrap() {
    if !e2e() {
        eprintln!("skipped: set ZAUN_E2E=1");
        return;
    }
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:300,detach:2500,exit:0",
            real_bwrap: true,
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Exited { code: 0 },
                ..
            }
        ),
        "{out:?}"
    );
    let s = ended(&w, id);
    assert!(s.duration_ms.unwrap() >= 2700, "{:?}", s.duration_ms);
    assert!(s.pid_ns.is_some());
}

/// ADR-0029 through the supervisor on the real compositor and the real `bwrap`: the
/// restricted socket is registered before the sandbox starts and is bound inside it; the
/// compositor serves it while the session runs and stops once the sandbox is gone. Skipped
/// without a compositor that offers security contexts.
#[test]
fn e2e_restricted_wayland_socket_lives_with_the_session() {
    use zaun_core::host::wayland::globals;
    use zaun_core::host::{CompositorProbe, LiveCompositor};
    use zaun_core::sandbox::HostAction;
    if !e2e() {
        eprintln!("skipped: set ZAUN_E2E=1");
        return;
    }
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY").and_then(|name| {
        let p = PathBuf::from(&name);
        if p.is_absolute() {
            Some(p)
        } else {
            std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join(name))
        }
    }) else {
        eprintln!("skipped: no Wayland compositor");
        return;
    };
    if !LiveCompositor.offers_security_context(&display) {
        eprintln!("skipped: the compositor offers no security contexts");
        return;
    }
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let mut p = plan(
        &w,
        &PlanOpts {
            fake: "umu,game,sleep:1500,exit:0",
            real_bwrap: true,
            ..PlanOpts::default()
        },
    );
    let socket = p.paths.runtime_dir.join("wayland");
    if socket.as_os_str().len() > 107 {
        eprintln!(
            "skipped: {} is too long for a socket (set TMPDIR=/tmp)",
            socket.display()
        );
        return;
    }
    p.host_actions.push(HostAction::CreateWaylandContext {
        display,
        socket: socket.clone(),
        app_id: w.game.to_string(),
        instance_id: p.session.id.to_string(),
    });
    p.sandbox.args.extend([
        "--ro-bind".into(),
        socket.clone().into_os_string(),
        "/run/wayland-0".into(),
    ]);
    let r = start_with(
        &w,
        p,
        cfg(&w),
        Box::new(w.db()),
        Arc::new(SystemClock::new()),
    );
    // Served while the session runs. A second name for the socket outlives the session's
    // runtime directory, so the end can be checked too.
    let link = w.paths.data_dir.join("wayland-link");
    let deadline = Instant::now() + Duration::from_secs(10);
    while globals(&socket).is_err() {
        assert!(Instant::now() < deadline, "never served");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::hard_link(&socket, &link).unwrap();
    assert!(globals(&link).is_ok());
    let (out, _) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::Exited { code: 0 },
                ..
            }
        ),
        "{out:?}"
    );
    let gone = Instant::now() + Duration::from_secs(3);
    while globals(&link).is_ok() {
        assert!(Instant::now() < gone, "still served after the session");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn e2e_graceful_stop_reaches_setsid_grandchildren_under_real_bwrap() {
    if !e2e() {
        eprintln!("skipped: set ZAUN_E2E=1");
        return;
    }
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let r = start(
        &w,
        &PlanOpts {
            fake: "umu,children:10:60000,game,sleep:60000",
            real_bwrap: true,
            ..PlanOpts::default()
        },
    );
    let id = r.id();
    assert!(wait_state(
        &w,
        id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    let ns = w.db().session(id).unwrap().unwrap().pid_ns.unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(namespace_members(ns) >= 12);
    r.stop(StopMode::Graceful);
    let (out, wall) = r.join();
    assert!(
        matches!(
            out,
            SupervisorOutcome::Ended {
                reason: EndReason::StoppedByUser,
                ..
            }
        ),
        "{out:?}"
    );
    assert!(wall < Duration::from_secs(30));
    assert_eq!(namespace_members(ns), 0);
}

#[test]
fn e2e_killed_supervisor_takes_the_sandbox_down() {
    if !e2e() {
        eprintln!("skipped: set ZAUN_E2E=1");
        return;
    }
    let _s = serial();
    let _turn = helpers_turn();
    let w = World::new();
    let l = launcher(&w);
    let p = plan(
        &w,
        &PlanOpts {
            fake: "umu,children:4:60000,game,sleep:60000",
            real_bwrap: true,
            ..PlanOpts::default()
        },
    );
    let h = l.start(&p).unwrap();
    assert!(wait_state(
        &w,
        h.session_id,
        SessionState::Running,
        Duration::from_secs(5)
    ));
    let ns = w
        .db()
        .session(h.session_id)
        .unwrap()
        .unwrap()
        .pid_ns
        .unwrap();
    assert!(namespace_members(ns) >= 6);
    let proc = RealProc::new(Arc::new(RealFs::new()));
    zaun_core::session::sys::signal_identity(&proc, h.supervisor, Sig::Kill).unwrap();
    let t = Instant::now();
    while namespace_members(ns) > 0 && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        namespace_members(ns),
        0,
        "the sandbox died with its supervisor"
    );
    let mut db = w.db();
    let out = recover(
        &mut db,
        &proc,
        &RealFs::new(),
        &w.paths,
        &SystemClock::new(),
        &RecoveryOptions::default(),
    )
    .unwrap();
    assert_eq!(out[0].reason, EndReason::LauncherTerminated);
}
