//! AppService flows (SVC-002…SVC-008, PROC-011) driven headlessly: commands in, events out,
//! job completions pumped by hand. Real filesystem in a temporary installation, a fake
//! launcher and a scripted runtime service.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use easyplay_core::artwork::steam::FsAppIdFile;
use easyplay_core::artwork::{ArtIndex, NoIcons};
use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::config::Preferences;
use easyplay_core::error::ErrorCode;
use easyplay_core::fsx::{Fs, MountTable, RealFs};
use easyplay_core::host::StaticHostFacts;
use easyplay_core::identity::{FsPeInspector, SharedLocations};
use easyplay_core::ids::{GameId, RuntimeName};
use easyplay_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use easyplay_core::launch::{LaunchState, NoLocks};
use easyplay_core::library::RemoveOptions;
use easyplay_core::library::draft::NoUmuTitles;
use easyplay_core::library::service::NoPolicyCheck;
use easyplay_core::model::{Game, PromptAnswer, PromptKind, RuntimeRef, SlrVariant};
use easyplay_core::procfs::RealProc;
use easyplay_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use easyplay_core::service::session_watch::{self, SessionWatcher, WatchMsg};
use easyplay_core::service::{
    AfterCreate, AppService, Command, Event, LaunchEnv, LaunchOrigin, Notice, OpenOrigin,
    ServiceDeps,
};
use easyplay_core::storage::{Db, GameRepo, MigrateOptions, SessionClose, SessionRepo};
use easyplay_testkit::TestEnv;
use easyplay_testkit::fakes::FakeLauncher;
use easyplay_testkit::fixtures::{games, hosts, pe};

/// A runtime service whose every resolution needs a provision.
struct NeedsSlr(Arc<JobPool>);

impl RuntimeService for NeedsSlr {
    fn catalog(&self) -> Arc<RuntimeCatalog> {
        Arc::new(RuntimeCatalog::default())
    }
    fn refresh(&self) -> JobHandle<Arc<RuntimeCatalog>> {
        self.0
            .submit(JobPriority::Background, JobLabel("scan"), |_| {
                Arc::new(RuntimeCatalog::default())
            })
    }
    fn resolve(&self, _r: &RuntimeRef, _g: Option<&Game>) -> Resolution {
        Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant: SlrVariant::Steamrt3,
            build: RuntimeName::new("UMU-Proton-10.0-4").unwrap(),
        })
    }
}

struct World {
    env: TestEnv,
    svc: AppService,
    events: Receiver<Event>,
    launcher: Arc<FakeLauncher>,
}

fn deps(env: &TestEnv, launcher: Arc<FakeLauncher>) -> ServiceDeps {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
    ServiceDeps {
        fs: Arc::clone(&fs),
        clock: Arc::clone(&clock),
        paths: env.paths(),
        jobs: Arc::clone(&jobs),
        proc: Arc::new(RealProc::new(Arc::clone(&fs))),
        launcher,
        locks: Arc::new(NoLocks),
        runtimes: Arc::new(NeedsSlr(Arc::clone(&jobs))),
        host: Arc::new(StaticHostFacts(Arc::new(hosts::arch_mesa()))),
        pe: Arc::new(FsPeInspector::new(Arc::clone(&fs), Arc::clone(&clock))),
        icons: Arc::new(NoIcons),
        codec: None,
        launch_env: LaunchEnv {
            uid: 1000,
            gid: 1000,
            user_name: "sam".into(),
            group_name: "sam".into(),
            home: env.home(),
            host_env: BTreeMap::new(),
            init_binary: PathBuf::from("/nonexistent/easyplay-init"),
            mounts: MountTable::default(),
            user_dirs: Vec::new(),
            seccomp_available: false,
            global_env: Vec::new(),
        },
        prefs: Preferences::default(),
        http: None,
        app_id_file: Arc::new(FsAppIdFile::new(Arc::clone(&fs))),
        art_index: Arc::new(ArtIndex::new()),
        shared: SharedLocations::default(),
        policy_check: Some(Arc::new(NoPolicyCheck)),
        umu_titles: Arc::new(NoUmuTitles),
        watch_sessions: false,
    }
}

impl World {
    fn new() -> Self {
        let env = TestEnv::new().unwrap();
        let launcher = Arc::new(FakeLauncher::new());
        let (svc, events) = AppService::open_headless(deps(&env, Arc::clone(&launcher))).unwrap();
        let mut w = Self {
            env,
            svc,
            events,
            launcher,
        };
        w.settle();
        w.drain();
        w
    }

    /// Handle job completions until none arrives for a while.
    fn settle(&mut self) {
        while self.svc.pump(Duration::from_millis(300)) {}
    }

    fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    fn send(&mut self, c: Command) -> Vec<Event> {
        self.svc.handle(c);
        self.settle();
        self.drain()
    }

    fn exe(&self, dir: &str) -> PathBuf {
        let d = self.env.home().join("Games").join(dir);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(format!("{dir}.exe"));
        std::fs::write(&p, pe::sample_game()).unwrap();
        p
    }

    fn add(&mut self, dir: &str) -> GameId {
        let exe = self.exe(dir);
        let evs = self.send(Command::OpenPaths {
            paths: vec![exe],
            origin: OpenOrigin::Picker,
        });
        let draft = evs
            .into_iter()
            .find_map(|e| match e {
                Event::SetupRequested { draft, .. } => Some(draft),
                _ => None,
            })
            .expect("setup requested");
        let evs = self.send(Command::CreateGame {
            draft,
            then: AfterCreate::Nothing,
        });
        evs.iter()
            .find_map(|e| match e {
                Event::Notice(Notice::GameAdded { game, .. }) => Some(*game),
                _ => None,
            })
            .expect("game added")
    }
}

#[test]
fn open_unknown_then_create_then_open_again_shows_the_game() {
    let mut w = World::new();
    let id = w.add("Foo");
    assert!(w.svc.hub().library.load().get(id).is_some());
    let exe = w.env.home().join("Games/Foo/Foo.exe");
    // Dropped again: shown, not launched (LOGIC_SPECS §1 origin rule).
    let evs = w.send(Command::OpenPaths {
        paths: vec![exe.clone()],
        origin: OpenOrigin::DragDrop,
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::ShowGame(g) if *g == id))
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::AlreadyInLibrary { game }) if *game == id))
    );
    assert!(w.launcher.plans().is_empty());
}

#[test]
fn not_a_windows_program_is_a_failure() {
    let mut w = World::new();
    let p = w.env.home().join("notes.txt");
    std::fs::write(&p, vec![b'x'; 4096]).unwrap();
    let evs = w.send(Command::OpenPaths {
        paths: vec![p],
        origin: OpenOrigin::Picker,
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Failure(u) if u.code == ErrorCode::IdNotWindowsProgram))
    );
}

#[test]
fn setup_dialogs_are_shown_one_at_a_time() {
    let mut w = World::new();
    let a = w.exe("A");
    let b = w.exe("B");
    let evs = w.send(Command::OpenPaths {
        paths: vec![a, b],
        origin: OpenOrigin::Picker,
    });
    let shown = evs
        .iter()
        .filter(|e| matches!(e, Event::SetupRequested { .. }))
        .count();
    assert_eq!(shown, 1);
    let evs = w.send(Command::DiscardDraft);
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, Event::SetupRequested { .. }))
            .count(),
        1
    );
}

#[test]
fn launch_without_runtime_runs_a_trusted_session_first() {
    // M7: a runtime missing for a build umu cannot find by name is installed in a trusted
    // Provision session (network on, no game); the game itself is never planned when that
    // session cannot start.
    let mut w = World::new();
    let id = w.add("Bar");
    for _ in 0..2 {
        w.launcher.push(easyplay_testkit::FakeStart::Fails(
            ErrorCode::SesSupervisorNotStarted,
        ));
        let evs = w.send(Command::Launch {
            id,
            exe: None,
            origin: LaunchOrigin::Library,
        });
        let failed = evs
            .iter()
            .find_map(|e| match e {
                Event::LaunchProgress {
                    state: LaunchState::Failed(u),
                    ..
                } => Some(u.clone()),
                _ => None,
            })
            .expect("launch failed");
        assert_eq!(failed.code, ErrorCode::SesSupervisorNotStarted);
        assert!(evs.iter().any(|e| matches!(
            e,
            Event::LaunchProgress {
                state: LaunchState::Provisioning { .. },
                ..
            }
        )));
    }
    // The flow lock is released: the second launch was evaluated again.
    let plans = w.launcher.plans();
    assert_eq!(plans.len(), 2);
    for p in plans {
        assert_eq!(p.session.kind, easyplay_core::model::SessionKind::Provision);
        assert_eq!(p.session.game_id, None);
        assert_eq!(
            p.context.network,
            Some(easyplay_core::model::NetworkAccess::Full)
        );
    }
}

#[test]
fn favorite_and_remove() {
    let mut w = World::new();
    let id = w.add("Baz");
    w.send(Command::SetFavorite { id, favorite: true });
    assert!(w.svc.hub().library.load().get(id).unwrap().favorite);
    let evs = w.send(Command::RemoveGame {
        id,
        options: RemoveOptions::default(),
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::GameRemoved { .. })))
    );
    assert!(w.svc.hub().library.load().get(id).is_none());
    assert!(!w.env.paths().game_dir(id).exists());
}

#[test]
fn removing_a_running_game_asks_first() {
    let mut w = World::new();
    let id = w.add("Run");
    // A session row with this process as its "supervisor".
    let mut db = Db::open_rw(&w.env.paths().db_file).unwrap();
    let game = db.game(id).unwrap().unwrap();
    let mut s = games::session(&game, 1, 1, SystemClock::new().now());
    s.ended_at = None;
    s.end_reason = None;
    s.duration_ms = None;
    s.state = easyplay_core::model::SessionState::Running;
    let me = easyplay_core::session::sys::own_identity(&RealProc::new(Arc::new(RealFs::new())))
        .expect("own identity");
    s.supervisor = Some(easyplay_core::model::SupervisorIdent {
        pid: me.pid,
        start_ticks: me.start_time_ticks,
        boot_id: String::new(),
    });
    db.insert_session(&s).unwrap();
    drop(db);
    w.svc.poll_sessions_now();
    w.drain();
    let evs = w.send(Command::RemoveGame {
        id,
        options: RemoveOptions::default(),
    });
    let prompt = evs
        .iter()
        .find_map(|e| match e {
            Event::Prompt(p) => Some(p.clone()),
            _ => None,
        })
        .expect("prompt");
    assert_eq!(prompt.kind, PromptKind::RemoveRunningGame { game: id });
    // Cancel: nothing happens.
    w.send(Command::AnswerPrompt {
        prompt: prompt.id,
        answer: PromptAnswer::Cancel,
    });
    assert!(w.svc.hub().library.load().get(id).is_some());
}

#[test]
fn watcher_reports_open_and_ended_rows() {
    let env = TestEnv::new().unwrap();
    let paths = env.paths();
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    let mut db = Db::open_rw(&paths.db_file).unwrap();
    db.migrate(&MigrateOptions::default()).unwrap();
    let g = games::game(1, 1);
    db.insert_game(&g).unwrap();
    let now = SystemClock::new().now();
    let mut s = games::session(&g, 1, 1, now);
    s.ended_at = None;
    s.end_reason = None;
    s.duration_ms = None;
    s.state = easyplay_core::model::SessionState::Starting;
    db.insert_session(&s).unwrap();

    let mut tracked = std::collections::BTreeSet::from([s.id]);
    let p = session_watch::poll(&mut db, &mut tracked).unwrap();
    assert_eq!(p.open.len(), 1);
    assert!(p.ended.is_empty());

    db.close_session(&SessionClose {
        id: s.id,
        ended_at: now,
        duration_ms: 1000,
        end_reason: easyplay_core::model::EndReason::Exited { code: 0 },
        exit_status: Some(0),
        counted: false,
        pieces: Vec::new(),
    })
    .unwrap();
    let p = session_watch::poll(&mut db, &mut tracked).unwrap();
    assert!(p.open.is_empty());
    assert_eq!(p.ended.len(), 1);
    assert!(tracked.is_empty());

    // The thread: parked when idle, reports a tracked session's end.
    let (tx, rx) = std::sync::mpsc::channel();
    let w = SessionWatcher::spawn(paths.db_file.clone(), move |p| {
        let _ = tx.send(p);
    })
    .unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(1500)).is_err());
    w.send(WatchMsg::Track(s.id));
    let p = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(p.ended.len(), 1);
    drop(w);
}

#[test]
fn update_load_and_query_stats_answer_the_game_page() {
    use easyplay_core::ids::QueryId;
    use easyplay_core::service::{GameEdit, QueryResult};
    let mut w = World::new();
    let id = w.add("Qux");
    let mut policy = w.svc.library().game(id).unwrap().policy;
    policy.network = easyplay_core::model::NetworkAccess::Full;
    w.send(Command::UpdateGame {
        id,
        edits: vec![
            GameEdit::Name("Qux Deluxe".into()),
            GameEdit::Notes("note".into()),
            GameEdit::Policy(Box::new(policy)),
        ],
    });
    let q = QueryId::from_uuid(uuid::Uuid::from_u128(9));
    let evs = w.send(Command::LoadGame { id, reply_to: q });
    let game = evs
        .into_iter()
        .find_map(|e| match e {
            Event::QueryResult {
                id,
                result: QueryResult::Game(g),
            } if id == q => Some(g),
            _ => None,
        })
        .expect("game loaded");
    assert_eq!(game.name, "Qux Deluxe");
    assert_eq!(game.notes, "note");
    assert_eq!(
        game.policy.network,
        easyplay_core::model::NetworkAccess::Full
    );
    assert!(game.policy.preset.base() != easyplay_core::model::BasePreset::Offline);
    w.send(Command::UpdateGame {
        id,
        edits: vec![GameEdit::Preset(easyplay_core::model::BasePreset::Offline)],
    });
    assert_eq!(
        w.svc.library().game(id).unwrap().policy.preset,
        easyplay_core::model::Preset::Offline
    );
    let today = jiff::civil::date(2026, 11, 12);
    let evs = w.send(Command::QueryStats {
        request: easyplay_core::stats::StatsRequest::LibraryOverview {
            range: easyplay_core::stats::DateRange::last_days(today, 7),
        },
        reply_to: q,
    });
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::QueryResult {
            result: QueryResult::Stats(easyplay_core::stats::StatsResult::Library(_)),
            ..
        }
    )));
}
