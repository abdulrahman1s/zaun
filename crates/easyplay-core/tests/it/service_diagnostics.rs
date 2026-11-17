//! AppService diagnostics and Developer Mode flows (ERR-002…ERR-005, DEV-001, DEV-003):
//! recovery mode on a damaged or newer library, the launch inspector's dry run, the
//! diagnostics report, the crash banner, the full Doctor report. Same rig as
//! `service_flows_wave4.rs`.

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
use easyplay_core::library::draft::NoUmuTitles;
use easyplay_core::library::service::NoPolicyCheck;
use easyplay_core::model::{Game, RuntimeRef, SlrVariant};
use easyplay_core::procfs::RealProc;
use easyplay_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use easyplay_core::service::{
    AfterCreate, AppService, Command, Event, LaunchEnv, LaunchOrigin, Notice, OpenOrigin,
    ServiceDeps,
};
use easyplay_core::storage::{Db, SessionRepo};
use easyplay_testkit::TestEnv;
use easyplay_testkit::fakes::FakeLauncher;
use easyplay_testkit::fixtures::{hosts, pe};

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
        desktop: None,
        http: None,
        app_id_file: Arc::new(FsAppIdFile::new(Arc::clone(&fs))),
        art_index: Arc::new(ArtIndex::new()),
        shared: SharedLocations::default(),
        policy_check: Some(Arc::new(NoPolicyCheck)),
        umu_titles: Arc::new(NoUmuTitles),
        watch_sessions: false,
        chat: None,
    }
}

impl World {
    fn new() -> Self {
        Self::in_env(TestEnv::new().unwrap())
    }

    fn in_env(env: TestEnv) -> Self {
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

fn query(evs: &[Event], q: easyplay_core::ids::QueryId) -> easyplay_core::service::QueryResult {
    evs.iter()
        .find_map(|e| match e {
            Event::QueryResult { id, result } if *id == q => Some(result.clone()),
            _ => None,
        })
        .expect("query answered")
}

fn failure(evs: &[Event]) -> Option<ErrorCode> {
    evs.iter().find_map(|e| match e {
        Event::Failure(u) => Some(u.code),
        _ => None,
    })
}

fn recovery(w: &AppService) -> Option<easyplay_core::service::RecoveryState> {
    w.hub().diagnostics.load().recovery.clone()
}

fn game_count(w: &AppService) -> usize {
    w.hub().library.load().games.len()
}

/// Close the service (and its database) and keep the directories.
fn close(w: World) -> TestEnv {
    let World { env, svc, .. } = w;
    drop(svc);
    env
}

fn launch_refused(w: &mut World, id: GameId, code: ErrorCode) {
    let evs = w.send(Command::Launch {
        id,
        exe: None,
        origin: LaunchOrigin::Library,
    });
    let failed = evs.iter().find_map(|e| match e {
        Event::LaunchProgress {
            state: LaunchState::Failed(u),
            ..
        } => Some(u.code),
        _ => None,
    });
    assert_eq!(failed, Some(code), "{evs:?}");
    assert!(w.launcher.plans().is_empty());
}

/// ERR-004: a library file that is not a database any more starts recovery mode with the
/// backups listed; restoring one gives a working library again, and launching is refused
/// with the reason until then.
#[test]
fn damaged_library_restore_backup() {
    use easyplay_core::service::{DatabaseChoice, RecoveredHow, RecoveryReason};
    use easyplay_core::storage::backup::{self, BackupKind};
    let mut w = World::new();
    let a = w.add("Alpha");
    let paths = w.env.paths();
    let now = SystemClock::new().now();
    backup::create(
        w.svc.library().db_mut(),
        BackupKind::Daily,
        &paths.backups_dir,
        now.saturating_add(Duration::from_secs(90_000)),
    )
    .unwrap();
    let env = close(w);
    std::fs::write(&paths.db_file, vec![0x42u8; 64 * 1024]).unwrap();
    let _ = std::fs::remove_file(paths.db_file.with_extension("db-wal"));

    let mut w = World::in_env(env);
    let r = recovery(&w.svc).expect("recovery mode");
    assert_eq!(r.error.code, ErrorCode::DbCorrupt);
    let RecoveryReason::Damaged {
        backups,
        repaired_games,
        ..
    } = &r.reason
    else {
        panic!("{r:?}")
    };
    assert_eq!(*repaired_games, None);
    let (best, _, games) = backups.first().cloned().expect("a backup");
    assert_eq!(games, 1);
    assert_eq!(game_count(&w.svc), 0);
    launch_refused(&mut w, a, ErrorCode::DbCorrupt);
    // Writes are refused too.
    let evs = w.send(Command::SetFavorite {
        id: a,
        favorite: true,
    });
    assert_eq!(failure(&evs), Some(ErrorCode::DbCorrupt));

    let evs = w.send(Command::ResolveDatabase(DatabaseChoice::RestoreBackup(
        best,
    )));
    assert!(
        evs.iter().any(|e| matches!(
            e,
            Event::Notice(Notice::LibraryRecovered {
                how: RecoveredHow::Restored,
                ..
            })
        )),
        "{evs:?}"
    );
    assert!(recovery(&w.svc).is_none());
    assert_eq!(game_count(&w.svc), 1);
    // The damaged file was kept.
    let kept = std::fs::read_dir(&paths.backups_dir)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("replaced-"));
    assert!(kept);
    // And the library works: a favourite sticks.
    let evs = w.send(Command::SetFavorite {
        id: a,
        favorite: true,
    });
    assert_eq!(failure(&evs), None);
}

/// ERR-004: "Start with an empty library" keeps the damaged file and opens a fresh one;
/// a backup outside the backups folder is refused.
#[test]
fn damaged_library_start_empty() {
    use easyplay_core::service::{DatabaseChoice, RecoveredHow};
    let mut w = World::new();
    w.add("Alpha");
    let paths = w.env.paths();
    let env = close(w);
    std::fs::write(&paths.db_file, b"definitely not sqlite").unwrap();
    let mut w = World::in_env(env);
    assert!(recovery(&w.svc).is_some());
    let evs = w.send(Command::ResolveDatabase(DatabaseChoice::RestoreBackup(
        w.env.home().join("x.db"),
    )));
    assert_eq!(failure(&evs), Some(ErrorCode::FsReadDenied));
    assert!(recovery(&w.svc).is_some());
    let evs = w.send(Command::ResolveDatabase(DatabaseChoice::StartEmpty));
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::Notice(Notice::LibraryRecovered {
            how: RecoveredHow::StartedEmpty,
            ..
        })
    )));
    assert!(recovery(&w.svc).is_none());
    assert_eq!(game_count(&w.svc), 0);
    w.add("Gamma");
    assert_eq!(game_count(&w.svc), 1);
    let corrupt = std::fs::read_dir(&paths.backups_dir)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("corrupt-"));
    assert!(corrupt);
}

/// ERROR_HANDLING §4.13 / EP-DB-010: a library from a newer EasyPlay is left untouched and
/// the service runs in recovery mode.
#[test]
fn newer_library_is_not_touched() {
    let mut w = World::new();
    let a = w.add("Alpha");
    let paths = w.env.paths();
    let env = close(w);
    {
        let mut db = Db::open_rw(&paths.db_file).unwrap();
        db.checkpoint_truncate().unwrap();
        db.close().unwrap();
        // `user_version` is the big-endian u32 at offset 60 of the header.
        let mut bytes = std::fs::read(&paths.db_file).unwrap();
        bytes[60..64].copy_from_slice(&99u32.to_be_bytes());
        std::fs::write(&paths.db_file, bytes).unwrap();
    }
    let before = std::fs::read(&paths.db_file).unwrap();
    let mut w = World::in_env(env);
    let r = recovery(&w.svc).expect("recovery mode");
    assert_eq!(r.error.code, ErrorCode::DbNewerVersion);
    launch_refused(&mut w, a, ErrorCode::DbNewerVersion);
    let _env = close(w);
    let after = std::fs::read(&paths.db_file);
    assert_eq!(after.unwrap(), before);
}

/// DEV-001: the dry run answers with the plan of the step that would run first, and has no
/// side effects (no session row, nothing handed to the launcher).
#[test]
fn inspector_dry_run_has_no_side_effects() {
    use easyplay_core::ids::QueryId;
    use easyplay_core::launch::InspectorSource;
    use easyplay_core::service::{InspectTarget, QueryResult};
    let mut w = World::new();
    let g = w.add("Alpha");
    let q = QueryId::from_uuid(uuid::Uuid::from_u128(7));
    let evs = w.send(Command::Inspect {
        target: InspectTarget::NextLaunch { game: g, exe: None },
        reply_to: q,
    });
    let QueryResult::Inspector(d) = query(&evs, q) else {
        panic!("{evs:?}")
    };
    assert_eq!(d.source, InspectorSource::DryRun);
    assert!(d.sandbox.is_some());
    assert!(!d.plan.sandbox.args.is_empty());
    assert!(!d.notes.is_empty(), "the provision step is explained");
    assert!(w.launcher.plans().is_empty());
    assert!(
        w.svc
            .library()
            .db_mut()
            .session_history(None, &[], 0, 10)
            .unwrap()
            .is_empty()
    );
    // A session that does not exist answers with a failure.
    let q2 = QueryId::from_uuid(uuid::Uuid::from_u128(8));
    let evs = w.send(Command::Inspect {
        target: InspectTarget::Session(easyplay_testkit::fixtures::games::session_id(1, 1)),
        reply_to: q2,
    });
    assert!(matches!(query(&evs, q2), QueryResult::Failed(_)));
}

/// DEV-001: inspecting a session that does not exist answers with the session's not-found
/// code, not a file-access error.
#[test]
fn inspector_unknown_session_is_not_found() {
    use easyplay_core::ids::QueryId;
    use easyplay_core::service::{InspectTarget, QueryResult};
    let mut w = World::new();
    let q = QueryId::from_uuid(uuid::Uuid::from_u128(10));
    let evs = w.send(Command::Inspect {
        target: InspectTarget::Session(easyplay_testkit::fixtures::games::session_id(1, 1)),
        reply_to: q,
    });
    let QueryResult::Failed(u) = query(&evs, q) else {
        panic!("{evs:?}")
    };
    assert_eq!(u.code, ErrorCode::SesSessionNotFound);
}

/// ERR-003: the report is built for the preview and saved as an archive.
#[test]
fn report_build_and_save() {
    use easyplay_core::diagnostics::report;
    use easyplay_core::ids::QueryId;
    use easyplay_core::service::QueryResult;
    let mut w = World::new();
    let g = w.add("Alpha");
    let q = QueryId::from_uuid(uuid::Uuid::from_u128(9));
    let evs = w.send(Command::BuildReport {
        game: Some(g),
        session: None,
        reply_to: q,
    });
    let QueryResult::Report(files) = query(&evs, q) else {
        panic!("{evs:?}")
    };
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"version.txt") && names.contains(&"game/policy.json"));
    let home = w.env.home().display().to_string();
    for f in &files {
        assert!(
            !String::from_utf8_lossy(&f.bytes).contains(&home),
            "{} not redacted",
            f.name
        );
    }
    let dest = w.env.home().join("report.tar.gz");
    let evs = w.send(Command::SaveReport {
        files: files.clone(),
        dest: dest.clone(),
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::ReportSaved { .. })))
    );
    let entries = report::read_archive(&std::fs::read(&dest).unwrap()).unwrap();
    assert_eq!(entries.len(), files.len());
}

/// ERR-005: a crash record left by the last run shows as the banner until dismissed.
#[test]
fn crash_banner_until_dismissed() {
    use easyplay_core::diagnostics::crash::{self, CRASH_FORMAT, CrashPhase, CrashRecord};
    let env = TestEnv::new().unwrap();
    let dir = env.paths().crash_dir;
    let rec = CrashRecord {
        format: CRASH_FORMAT,
        at: SystemClock::new().now(),
        version: "0.1.0".into(),
        commit: "x".into(),
        role: "gui".into(),
        thread: "app-service".into(),
        message: "boom".into(),
        location: None,
        backtrace: String::new(),
        uptime_ms: 60_000,
        phase: CrashPhase::Running,
        log_tail: Vec::new(),
    };
    crash::write(&RealFs::new(), &dir, &rec).unwrap();
    let mut w = World::in_env(env);
    assert_eq!(w.svc.hub().diagnostics.load().crash.as_ref(), Some(&rec));
    w.send(Command::DismissCrash);
    assert!(w.svc.hub().diagnostics.load().crash.is_none());
    assert!(crash::pending(&RealFs::new(), &dir).is_none());
}

/// ERR-002: the full Doctor report arrives after the host facts, with the database check.
#[test]
fn doctor_report_after_host_facts() {
    let mut w = World::new();
    w.send(Command::RunDoctor);
    let d = w.svc.hub().diagnostics.load();
    let r = d.doctor.as_ref().expect("doctor report");
    let d03 = r.checks.iter().find(|c| c.id == "D-03").unwrap();
    assert_eq!(
        d03.status,
        easyplay_core::host::CheckStatus::Pass,
        "{d03:?}"
    );
    assert!(r.checks.iter().any(|c| c.id == "D-10"));
}

/// DEV-006: with Developer Mode off a dangerous variable cannot be added; with it on it
/// can; turned off again, the saved variable keeps the game editable (and launchable) while
/// a further one is refused.
#[test]
fn unsafe_options_are_gated_but_saved_ones_keep_working() {
    use easyplay_core::config::PreferencePatch;
    use easyplay_core::model::EnvVar;
    use easyplay_core::service::GameEdit;
    let env = TestEnv::new().unwrap();
    let launcher = Arc::new(FakeLauncher::new());
    let mut d = deps(&env, Arc::clone(&launcher));
    d.policy_check = None;
    let (svc, events) = AppService::open_headless(d).unwrap();
    let mut w = World {
        env,
        svc,
        events,
        launcher,
    };
    w.settle();
    w.drain();
    let g = w.add("Alpha");
    let game = w.svc.library().game(g).unwrap();
    let with = |names: &[&str]| {
        let mut l = game.launch.clone();
        l.env = names
            .iter()
            .map(|n| EnvVar {
                name: (*n).into(),
                value: "/opt/x.so".into(),
            })
            .collect();
        Command::UpdateGame {
            id: g,
            edits: vec![GameEdit::Launch(Box::new(l))],
        }
    };
    let dev = |on: bool| {
        Command::SetPreference(PreferencePatch::new().set("advanced.developer_mode", on))
    };
    let evs = w.send(with(&["LD_PRELOAD"]));
    assert!(failure(&evs).is_some(), "added without Developer Mode");
    w.send(dev(true));
    let evs = w.send(with(&["LD_PRELOAD"]));
    assert_eq!(failure(&evs), None, "{evs:?}");
    w.send(dev(false));
    let evs = w.send(Command::UpdateGame {
        id: g,
        edits: vec![GameEdit::Name("Alpha Renamed".into())],
    });
    assert_eq!(failure(&evs), None, "saved option blocks editing: {evs:?}");
    let evs = w.send(with(&["LD_PRELOAD", "LD_LIBRARY_PATH"]));
    assert!(failure(&evs).is_some(), "a second one was accepted");
    let saved = w.svc.library().game(g).unwrap();
    assert_eq!(saved.launch.env.len(), 1);
    assert_eq!(
        easyplay_core::sandbox::validate::advanced_options(&saved.policy, &saved.launch).len(),
        1
    );
}
