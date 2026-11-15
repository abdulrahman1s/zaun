//! The GUI over the real `AppService` (throw-away installation, fake launcher): the Protection
//! tab shows core's `explain_policy` statements, per-program arguments and labels edited in
//! the Launch options tab reach the database, and statistics edits answer.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::artwork::steam::FsAppIdFile;
use easyplay_core::artwork::{ArtIndex, NoIcons};
use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::config::Preferences;
use easyplay_core::fsx::{Fs, MountTable, RealFs};
use easyplay_core::host::StaticHostFacts;
use easyplay_core::identity::{FsPeInspector, SharedLocations};
use easyplay_core::ids::GameId;
use easyplay_core::ids::RuntimeName;
use easyplay_core::jobs::{JobHandle, JobLabel, JobPool, JobPoolConfig, JobPriority};
use easyplay_core::launch::NoLocks;
use easyplay_core::library::draft::NoUmuTitles;
use easyplay_core::library::service::NoPolicyCheck;
use easyplay_core::model::SlrVariant;
use easyplay_core::model::{Game, RuntimeRef};
use easyplay_core::procfs::RealProc;
use easyplay_core::runtime::{ProvisionNeed, Resolution, RuntimeCatalog, RuntimeService};
use easyplay_core::service::{
    AfterCreate, AppService, Command, Event, LaunchEnv, Notice, OpenOrigin, ServiceDeps,
};
use easyplay_core::storage::{ExecutableRepo, ReadDb};
use easyplay_testkit::fakes::FakeLauncher;
use easyplay_testkit::fixtures::{hosts, pe};
use easyplay_testkit::{FakeClock, TestEnv};
use easyplay_ui::EasyPlayApp;
use easyplay_ui::bridge::UiBridge;
use easyplay_ui::route::Route;
use easyplay_ui::service_backend::ServiceBackend;
use easyplay_ui::textures::synthetic;
use easyplay_ui::theme::{self, Appearance};
use easyplay_ui::vm::details::DetailsTab;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

struct NoRuntimes(Arc<JobPool>);

impl RuntimeService for NoRuntimes {
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

fn deps(env: &TestEnv) -> ServiceDeps {
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let jobs = Arc::new(JobPool::new(JobPoolConfig::default(), Arc::clone(&clock)).unwrap());
    ServiceDeps {
        fs: Arc::clone(&fs),
        clock: Arc::clone(&clock),
        paths: env.paths(),
        jobs: Arc::clone(&jobs),
        proc: Arc::new(RealProc::new(Arc::clone(&fs))),
        launcher: Arc::new(FakeLauncher::new()),
        locks: Arc::new(NoLocks),
        runtimes: Arc::new(NoRuntimes(Arc::clone(&jobs))),
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
            seccomp_available: true,
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
    }
}

/// Add a game headlessly, before the GUI's service starts.
fn add_game(env: &TestEnv, dir: &str) -> (GameId, PathBuf) {
    let d = env.home().join("Games").join(dir);
    std::fs::create_dir_all(&d).unwrap();
    let exe = d.join(format!("{dir}.exe"));
    std::fs::write(&exe, pe::sample_game()).unwrap();
    let (mut svc, events) = AppService::open_headless(deps(env)).unwrap();
    let settle = |svc: &mut AppService| while svc.pump(Duration::from_millis(300)) {};
    settle(&mut svc);
    svc.handle(Command::OpenPaths {
        paths: vec![exe.clone()],
        origin: OpenOrigin::Picker,
    });
    settle(&mut svc);
    let draft = events
        .try_iter()
        .find_map(|e| match e {
            Event::SetupRequested { draft, .. } => Some(draft),
            _ => None,
        })
        .expect("draft");
    svc.handle(Command::CreateGame {
        draft,
        then: AfterCreate::Nothing,
    });
    settle(&mut svc);
    let id = events
        .try_iter()
        .find_map(|e| match e {
            Event::Notice(Notice::GameAdded { game, .. }) => Some(game),
            _ => None,
        })
        .expect("added");
    (id, exe)
}

fn harness(env: &TestEnv, id: GameId) -> Harness<'static, EasyPlayApp> {
    let (handle, events) = AppService::start(deps(env)).unwrap();
    let backend = ServiceBackend::new(handle, events).with_user_dirs(env.home(), Vec::new());
    let appearance = Appearance::default();
    let app = EasyPlayApp::new(
        UiBridge::new(Box::new(backend)),
        Arc::new(FakeClock::new(SystemClock::new().now())),
        appearance,
        Route::Game(id),
        0,
    )
    .with_art(Box::new(|_| Box::new(synthetic::Inline::new())));
    Harness::builder()
        .with_size([1280.0, 1400.0])
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app)
}

/// Step the GUI (the service answers on its own thread) until `done`.
fn pump(
    h: &mut Harness<'static, EasyPlayApp>,
    what: &str,
    done: impl Fn(&Harness<'static, EasyPlayApp>) -> bool,
) {
    for _ in 0..100 {
        h.step();
        if done(h) {
            h.step();
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn has_label(h: &Harness<'static, EasyPlayApp>, text: &str) -> bool {
    h.query_by_label_contains(text).is_some()
}

#[test]
fn protection_tab_explains_the_real_policy() {
    let env = TestEnv::new().unwrap();
    let (id, _) = add_game(&env, "Lantern");
    let mut h = harness(&env, id);
    h.state_mut().state.details.tab = DetailsTab::Protection;
    pump(&mut h, "the record", |h| {
        h.state().state.details.record().is_some()
    });
    pump(&mut h, "the permissions", |h| {
        has_label(h, "Internet and local network")
    });
    assert!(has_label(&h, "Game folder: Can read and change"));
    assert!(has_label(&h, "Save data: Private storage"));
    // Statements of the real planner: the private storage and the hardware masks.
    let a = &h
        .state()
        .state
        .details
        .protection
        .as_ref()
        .unwrap()
        .assessment;
    for key in [
        "access.own.private_storage",
        "access.identity.hardware_masked",
    ] {
        assert!(
            a.statements.iter().any(|s| s.key == key),
            "{key}: {:?}",
            a.statements.iter().map(|s| &s.key).collect::<Vec<_>>()
        );
    }
}

#[test]
fn program_arguments_and_label_are_saved_through_the_service() {
    let env = TestEnv::new().unwrap();
    let (id, exe) = add_game(&env, "Beacon");
    let mut h = harness(&env, id);
    h.state_mut().state.details.tab = DetailsTab::LaunchOptions;
    pump(&mut h, "the record", |h| {
        h.state().state.details.record().is_some()
    });
    let file = "Beacon.exe";
    h.get_by_label(&format!("Actions for {file}")).click();
    h.run_steps(2);
    h.get_by_label("Arguments for this program…").click();
    h.run_steps(2);
    let field = h.get_by_label("Arguments for this program");
    field.focus();
    field.type_text("-dx12 -windowed");
    h.run_steps(2);
    h.get_all_by_role_and_label(Role::Button, "Save")
        .last()
        .expect("Save")
        .click();
    h.run_steps(2);
    pump(&mut h, "the own-arguments line", |h| {
        has_label(h, "Own arguments: -dx12 -windowed")
    });
    let mut db = ReadDb::open(&env.paths().db_file).unwrap();
    let (_, row) = db.executable_by_canonical_path(&exe).unwrap().unwrap();
    assert_eq!(
        row.args_override,
        Some(vec!["-dx12".to_owned(), "-windowed".to_owned()])
    );

    h.get_by_label(&format!("Actions for {file}")).click();
    h.run_steps(2);
    h.get_by_label("Rename label…").click();
    h.run_steps(2);
    let field = h.get_by_label("Label");
    field.focus();
    field.type_text("DX12 build");
    h.run_steps(2);
    h.get_all_by_role_and_label(Role::Button, "Save")
        .last()
        .expect("Save")
        .click();
    h.run_steps(2);
    pump(&mut h, "the label", |h| has_label(h, "DX12 build"));
    let (_, row) = db.executable_by_canonical_path(&exe).unwrap().unwrap();
    assert_eq!(row.label.as_deref(), Some("DX12 build"));
}

/// The Doctor's rows are the checks of the service's full report (the bridge test
/// `a_new_doctor_report_refreshes_the_host_view` covers a report that arrives late).
#[test]
fn doctor_rows_are_the_reports_checks() {
    let env = TestEnv::new().unwrap();
    let (id, _) = add_game(&env, "Checkup");
    let mut h = harness(&env, id);
    pump(&mut h, "the Doctor report", |h| {
        h.state().bridge().diagnostics().doctor.is_some()
    });
    let bridge = h.state().bridge();
    let report = bridge.diagnostics().doctor.clone().unwrap();
    let rows: Vec<&str> = bridge.host().checks.iter().map(|c| &*c.id).collect();
    let checks: Vec<&str> = report.checks.iter().map(|c| &*c.id).collect();
    assert!(checks.len() > 2, "{checks:?}");
    assert_eq!(rows, checks);
}
