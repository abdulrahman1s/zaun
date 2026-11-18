//! AppService flows of wave 4 (SVC-005 tools and cancel, SVC-007 set up as new, SVC-010
//! maintenance, folder scan, prefix size, program edits, statistics edits) driven headlessly.
//! Same rig as `service_flows.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use zaun_core::artwork::steam::FsAppIdFile;
use zaun_core::artwork::{ArtIndex, NoIcons};
use zaun_core::clock::{Clock, SystemClock};
use zaun_core::config::Preferences;
use zaun_core::error::ErrorCode;
use zaun_core::fsx::{Fs, MountTable, RealFs};
use zaun_core::host::StaticHostFacts;
use zaun_core::identity::{FsPeInspector, SharedLocations};
use zaun_core::ids::{GameId, RuntimeName};
use zaun_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use zaun_core::launch::{LaunchState, NoLocks};
use zaun_core::library::draft::NoUmuTitles;
use zaun_core::library::service::NoPolicyCheck;
use zaun_core::model::{Game, PromptAnswer, PromptKind, RuntimeRef, SlrVariant};
use zaun_core::procfs::RealProc;
use zaun_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use zaun_core::service::{
    AfterCreate, AppService, Command, Event, LaunchEnv, LaunchOrigin, Notice, OpenOrigin,
    ServiceDeps,
};
use zaun_core::storage::{Db, GameRepo, SessionRepo};
use zaun_testkit::TestEnv;
use zaun_testkit::fakes::FakeLauncher;
use zaun_testkit::fixtures::{games, hosts, pe};

/// A runtime service with an empty catalogue whose every resolution is the same.
struct FixedRuntimes(Arc<JobPool>, Resolution);

impl RuntimeService for FixedRuntimes {
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
        self.1.clone()
    }
}

/// Every resolution needs a provision.
fn needs_slr() -> Resolution {
    Resolution::NeedsProvision(ProvisionNeed::Slr {
        variant: SlrVariant::Steamrt3,
        build: RuntimeName::new("UMU-Proton-10.0-4").unwrap(),
    })
}

struct World {
    env: TestEnv,
    svc: AppService,
    events: Receiver<Event>,
    launcher: Arc<FakeLauncher>,
}

fn deps(env: &TestEnv, launcher: Arc<FakeLauncher>, resolution: Resolution) -> ServiceDeps {
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
        runtimes: Arc::new(FixedRuntimes(Arc::clone(&jobs), resolution)),
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
            init_binary: PathBuf::from("/nonexistent/zaun-init"),
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
        overlay: None,
        media: None,
    }
}

impl World {
    fn new() -> Self {
        Self::resolving(needs_slr())
    }

    /// A service whose runtime resolutions are all `resolution`.
    fn resolving(resolution: Resolution) -> Self {
        let env = TestEnv::new().unwrap();
        let launcher = Arc::new(FakeLauncher::new());
        let (svc, events) =
            AppService::open_headless(deps(&env, Arc::clone(&launcher), resolution)).unwrap();
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

fn query(evs: &[Event], q: zaun_core::ids::QueryId) -> zaun_core::service::QueryResult {
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

#[test]
fn maintenance_takes_the_daily_backup_at_start() {
    let w = World::new();
    let backups: Vec<_> = std::fs::read_dir(&w.env.paths().backups_dir)
        .map(|d| d.flatten().collect())
        .unwrap_or_default();
    assert!(!backups.is_empty(), "no daily backup");
}

#[test]
fn scan_folder_lists_programs_best_first() {
    use zaun_core::ids::QueryId;
    use zaun_core::model::ExeRole;
    use zaun_core::service::QueryResult;
    let mut w = World::new();
    let dir = w.env.home().join("Dropped");
    std::fs::create_dir_all(dir.join("bin/sub")).unwrap();
    std::fs::write(dir.join("Game.exe"), vec![0u8; 5000]).unwrap();
    std::fs::write(dir.join("bin/Small.exe"), vec![0u8; 100]).unwrap();
    std::fs::write(dir.join("unins000.exe"), vec![0u8; 9000]).unwrap();
    std::fs::write(dir.join("setup.exe"), vec![0u8; 9000]).unwrap();
    std::fs::write(dir.join("readme.txt"), b"hi").unwrap();
    let q = QueryId::from_uuid(uuid::Uuid::from_u128(1));
    let evs = w.send(Command::ScanFolder {
        folder: dir.clone(),
        reply_to: q,
    });
    let QueryResult::Folder(f) = query(&evs, q) else {
        panic!("not a folder scan")
    };
    let names: Vec<String> = f
        .programs
        .iter()
        .map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["Game.exe", "Small.exe", "setup.exe", "unins000.exe"]
    );
    assert_eq!(f.programs[0].role, ExeRole::Primary);
    assert_eq!(f.programs[2].role, ExeRole::Installer);
    assert!(f.programs[3].filtered);
    assert!(f.error.is_none());

    let q2 = QueryId::from_uuid(uuid::Uuid::from_u128(2));
    let evs = w.send(Command::ScanFolder {
        folder: dir.join("nope"),
        reply_to: q2,
    });
    let QueryResult::Folder(f) = query(&evs, q2) else {
        panic!("not a folder scan")
    };
    assert!(f.error.is_some());
}

#[test]
fn prefix_is_measured_and_reset_off_the_service_thread() {
    use zaun_core::ids::QueryId;
    use zaun_core::library::ResetOptions;
    use zaun_core::service::QueryResult;
    let mut w = World::new();
    let id = w.add("Measure");
    let prefix = w.env.paths().prefix_dir(id);
    std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
    std::fs::write(prefix.join("drive_c/big.bin"), vec![1u8; 10_000]).unwrap();
    let q = QueryId::from_uuid(uuid::Uuid::from_u128(3));
    let evs = w.send(Command::MeasurePrefix { id, reply_to: q });
    let QueryResult::PrefixSize { bytes, complete } = query(&evs, q) else {
        panic!("no size")
    };
    assert!(bytes >= 10_000 && complete);

    let evs = w.send(Command::ResetPrefix {
        id,
        options: ResetOptions::default(),
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::PrefixReset { game }) if *game == id))
    );
    assert!(!prefix.join("drive_c/big.bin").exists());
    // The flow lock was released.
    w.send(Command::SetFavorite { id, favorite: true });
    assert!(w.svc.hub().library.load().get(id).unwrap().favorite);
}

#[test]
fn programs_are_added_edited_and_removed() {
    use zaun_core::model::ExeRole;
    use zaun_core::service::GameEdit;
    let mut w = World::new();
    let id = w.add("Multi");
    let second = w.env.home().join("Games/Multi/Launcher.exe");
    let mut bytes = pe::sample_game();
    bytes.extend_from_slice(b"different");
    std::fs::write(&second, bytes).unwrap();
    let evs = w.send(Command::AddExecutable {
        id,
        path: second.clone(),
        role: ExeRole::Alternate,
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::ProgramAdded { game }) if *game == id))
    );
    let g = w.svc.library().game(id).unwrap();
    assert_eq!(g.executables.len(), 2);
    let first = g.primary().unwrap().id;
    let other = g.executables.iter().find(|e| e.id != first).unwrap().id;
    w.send(Command::UpdateGame {
        id,
        edits: vec![
            GameEdit::Name("Renamed".into()),
            GameEdit::ExeLabel(other, Some("DX12 build".into())),
            GameEdit::ExeArgs(other, Some(vec!["-dx12".into()])),
            GameEdit::ExeRole(other, ExeRole::Primary),
        ],
    });
    let g = w.svc.library().game(id).unwrap();
    assert_eq!(g.name, "Renamed");
    let o = g.executables.iter().find(|e| e.id == other).unwrap();
    assert_eq!(o.role, ExeRole::Primary);
    assert_eq!(o.label.as_deref(), Some("DX12 build"));
    assert_eq!(o.args_override.as_deref(), Some(&["-dx12".to_owned()][..]));
    // The old primary is an alternate now and can be removed; the primary cannot.
    let evs = w.send(Command::UpdateGame {
        id,
        edits: vec![GameEdit::RemoveExe(other)],
    });
    assert!(failure(&evs).is_some());
    w.send(Command::UpdateGame {
        id,
        edits: vec![GameEdit::RemoveExe(first), GameEdit::ResetName],
    });
    let g = w.svc.library().game(id).unwrap();
    assert_eq!(g.executables.len(), 1);
    assert!(!g.name_is_user_set);
    assert_ne!(g.name, "Renamed");
}

#[test]
fn locate_relinks_a_moved_program() {
    let mut w = World::new();
    let id = w.add("Moving");
    let old = w.env.home().join("Games/Moving");
    let new = w.env.home().join("Elsewhere/Moving");
    std::fs::create_dir_all(new.parent().unwrap()).unwrap();
    std::fs::rename(&old, &new).unwrap();
    let evs = w.send(Command::LocateExecutable {
        id,
        exe: None,
        new_path: new.join("Moving.exe"),
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::ProgramLocated { game }) if *game == id)),
        "{evs:?}"
    );
    let g = w.svc.library().game(id).unwrap();
    assert_eq!(g.root.as_deref(), Some(new.as_path()));
}

#[test]
fn set_up_as_new_detaches_a_changed_program_and_drafts_it() {
    let mut w = World::new();
    let id = w.add("Changed");
    let exe = w.env.home().join("Games/Changed/Changed.exe");
    let other = pe::PeBuilder::new()
        .string("ProductName", "Another Game")
        .string("CompanyName", "Someone Else")
        .string("OriginalFilename", "another.exe")
        .import("KERNEL32.dll")
        .build();
    std::fs::write(&exe, other).unwrap();
    let evs = w.send(Command::OpenPaths {
        paths: vec![exe.clone()],
        origin: OpenOrigin::Picker,
    });
    let prompt = evs
        .iter()
        .find_map(|e| match e {
            Event::Prompt(p) => Some(p.clone()),
            _ => None,
        })
        .expect("ExeChanged prompt");
    assert!(matches!(prompt.kind, PromptKind::ExeChanged { .. }));
    let evs = w.send(Command::AnswerPrompt {
        prompt: prompt.id,
        answer: PromptAnswer::SetUpAsNew,
    });
    assert!(failure(&evs).is_none(), "{evs:?}");
    let draft = evs
        .into_iter()
        .find_map(|e| match e {
            Event::SetupRequested { draft, .. } => Some(draft),
            _ => None,
        })
        .expect("draft");
    // The old game keeps its record; its program is no longer at that path.
    let old = w.svc.library().game(id).unwrap();
    assert_ne!(old.primary().unwrap().canonical_path, exe);
    let evs = w.send(Command::CreateGame {
        draft,
        then: AfterCreate::Nothing,
    });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::GameAdded { .. }))),
        "{evs:?}"
    );
}

#[test]
fn answering_update_for_a_removed_program_reports_exe_not_found() {
    let mut w = World::new();
    let id = w.add("Vanishing");
    let exe = w.env.home().join("Games/Vanishing/Vanishing.exe");
    let other = pe::PeBuilder::new()
        .string("ProductName", "Another Game")
        .string("CompanyName", "Someone Else")
        .string("OriginalFilename", "another.exe")
        .import("KERNEL32.dll")
        .build();
    std::fs::write(&exe, other).unwrap();
    let evs = w.send(Command::OpenPaths {
        paths: vec![exe],
        origin: OpenOrigin::Picker,
    });
    let prompt = evs
        .iter()
        .find_map(|e| match e {
            Event::Prompt(p) => Some(p.clone()),
            _ => None,
        })
        .expect("ExeChanged prompt");
    // The program is gone by the time the user answers.
    let evs = w.send(Command::RemoveGame {
        id,
        options: zaun_core::library::RemoveOptions::default(),
    });
    assert!(failure(&evs).is_none(), "{evs:?}");
    let evs = w.send(Command::AnswerPrompt {
        prompt: prompt.id,
        answer: PromptAnswer::ItsAnUpdate,
    });
    assert_eq!(failure(&evs), Some(ErrorCode::LibExeNotFound), "{evs:?}");
}

#[test]
fn statistics_edits_delete_history() {
    use zaun_core::model::{EndReason, SessionState};
    let mut w = World::new();
    let id = w.add("Stats");
    let mut db = Db::open_rw(&w.env.paths().db_file).unwrap();
    let game = db.game(id).unwrap().unwrap();
    let now = SystemClock::new().now();
    let mut ids = Vec::new();
    for k in 1..=3 {
        let mut s = games::session(&game, 7, k, now);
        s.state = SessionState::Ended;
        s.ended_at = Some(now);
        s.duration_ms = Some(600_000);
        s.end_reason = Some(EndReason::Exited { code: 0 });
        s.counted = true;
        db.insert_session(&s).unwrap();
        ids.push(s.id);
    }
    zaun_core::stats::rebuild::rebuild(&mut db, Default::default()).unwrap();
    drop(db);
    w.svc.library().refresh_game(id).unwrap();
    let evs = w.send(Command::RemoveSession { id: ids[0] });
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::SessionRemoved { .. })))
    );
    assert_eq!(w.svc.hub().library.load().get(id).unwrap().session_count, 2);
    let evs = w.send(Command::ResetGameStatistics { id });
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::Notice(Notice::StatisticsCleared { game: Some(g), sessions: 2 }) if *g == id
    )));
    assert_eq!(w.svc.hub().library.load().get(id).unwrap().session_count, 0);
    let evs = w.send(Command::ClearStatistics);
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::Notice(Notice::StatisticsCleared { game: None, .. })
    )));
}

#[test]
fn tools_need_a_runtime_and_winetricks_asks_for_the_network() {
    use zaun_core::model::Tool;
    let mut w = World::new();
    let id = w.add("Tooly");
    let evs = w.send(Command::RunTool {
        id,
        tool: Tool::Winecfg,
    });
    assert_eq!(failure(&evs), Some(ErrorCode::RtNotInstalled));
    let evs = w.send(Command::RunTool {
        id,
        tool: Tool::Winetricks {
            verbs: vec!["corefonts".into()],
        },
    });
    let prompt = evs
        .iter()
        .find_map(|e| match e {
            Event::Prompt(p) => Some(p.clone()),
            _ => None,
        })
        .expect("ToolNeedsNetwork");
    assert!(matches!(prompt.kind, PromptKind::ToolNeedsNetwork { .. }));
    let evs = w.send(Command::AnswerPrompt {
        prompt: prompt.id,
        answer: PromptAnswer::AllowNetworkOnce,
    });
    assert_eq!(failure(&evs), Some(ErrorCode::RtNotInstalled));
    assert!(w.launcher.plans().is_empty());
}

#[test]
fn a_cancelled_launch_reports_cancelled_and_nothing_else() {
    let mut w = World::new();
    let id = w.add("Cancel");
    w.svc.handle(Command::Launch {
        id,
        exe: None,
        origin: LaunchOrigin::Library,
    });
    w.svc.handle(Command::CancelLaunch { id });
    w.settle();
    let evs = w.drain();
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::LaunchProgress {
            state: LaunchState::Cancelled,
            ..
        }
    )));
    assert!(!evs.iter().any(|e| matches!(
        e,
        Event::LaunchProgress {
            state: LaunchState::Failed(_),
            ..
        }
    )));
    // Released: the next launch is evaluated.
    let evs = w.send(Command::Launch {
        id,
        exe: None,
        origin: LaunchOrigin::Library,
    });
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::LaunchProgress {
            state: LaunchState::Failed(_),
            ..
        }
    )));
}

fn prompt(evs: &[Event]) -> Option<zaun_core::model::PromptRequest> {
    evs.iter().find_map(|e| match e {
        Event::Prompt(p) => Some(p.clone()),
        _ => None,
    })
}

#[test]
fn a_launch_that_stops_to_ask_is_over_for_the_ui() {
    use zaun_core::model::Activity;
    use zaun_core::runtime::UnavailableReason;
    // The game's compatibility layer is gone: the launch stops to ask which one to use.
    let gone = UnavailableReason::UnknownPinned(RuntimeName::new("GE-Proton9-1").unwrap());
    let mut w = World::resolving(Resolution::Unavailable(gone));
    let id = w.add("Asked");
    let evs = w.send(Command::Launch {
        id,
        exe: None,
        origin: LaunchOrigin::Library,
    });
    let asked = prompt(&evs).unwrap_or_else(|| panic!("no prompt: {evs:?}"));
    assert!(matches!(asked.kind, PromptKind::RuntimeUnavailable { .. }));
    // Without a final state the game page would show "Checking…" behind the question, and
    // for good after *Cancel*.
    let last = evs
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::LaunchProgress { game, state, .. } if *game == id => Some(state.clone()),
            _ => None,
        })
        .expect("launch progress");
    assert_eq!(last, LaunchState::Cancelled, "{evs:?}");
    assert_eq!(
        w.svc.hub().library.load().get(id).unwrap().activity,
        Activity::Idle
    );
    let evs = w.send(Command::AnswerPrompt {
        prompt: asked.id,
        answer: PromptAnswer::Cancel,
    });
    assert!(evs.is_empty(), "{evs:?}");
}

#[test]
fn stop_and_remove_cancels_a_launch_that_is_still_starting() {
    let mut w = World::new();
    let id = w.add("Starting");
    w.svc.handle(Command::Launch {
        id,
        exe: None,
        origin: LaunchOrigin::Library,
    });
    w.svc.handle(Command::RemoveGame {
        id,
        options: zaun_core::library::RemoveOptions::default(),
    });
    let asked = prompt(&w.drain()).expect("RemoveRunningGame prompt");
    assert!(matches!(asked.kind, PromptKind::RemoveRunningGame { .. }));
    w.svc.handle(Command::AnswerPrompt {
        prompt: asked.id,
        answer: PromptAnswer::StopAndRemove,
    });
    // The cancelled launch job ends on its own time.
    let mut evs = Vec::new();
    for _ in 0..100 {
        w.settle();
        evs.extend(w.drain());
        if w.svc.hub().library.load().get(id).is_none() {
            break;
        }
    }
    // Not asked again: the launch is cancelled and the game goes once its job has ended.
    assert!(prompt(&evs).is_none(), "{evs:?}");
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Notice(Notice::GameRemoved { .. }))),
        "{evs:?}"
    );
    assert!(w.svc.hub().library.load().get(id).is_none());
}

#[test]
fn what_a_games_files_tell_is_answered_off_the_service_thread() {
    use zaun_core::identity::Engine;
    use zaun_core::ids::QueryId;
    use zaun_core::service::QueryResult;
    let mut w = World::new();
    let id = w.add("About");
    let game = w
        .svc
        .hub()
        .library
        .load()
        .get(id)
        .unwrap()
        .primary_exe_path
        .clone();
    let dir = game.parent().unwrap();
    std::fs::write(dir.join("data.win"), b"").unwrap();
    let q = QueryId::from_uuid(uuid::Uuid::from_u128(4));
    let evs = w.send(Command::AboutGame { id, reply_to: q });
    let QueryResult::About(a) = query(&evs, q) else {
        panic!("no answer")
    };
    assert_eq!(a.engine.map(|e| e.engine), Some(Engine::GameMaker));
}
