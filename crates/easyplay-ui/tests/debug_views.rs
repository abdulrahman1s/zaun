//! Developer Mode views and diagnostics (DEV-002…DEV-004, DEV-006, ERR-002…ERR-005): the
//! launch inspector over a real planned launch, the log viewer with hostile content, the
//! report preview, the Doctor section, the crash banner and the damaged-library state, with
//! snapshots.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};

use common::{game_id, rig, settle, settled_library};
use easyplay_core::clock::{MonoInstant, Timestamp};
use easyplay_core::diagnostics::crash::{CRASH_FORMAT, CrashPhase, CrashRecord};
use easyplay_core::diagnostics::report::ReportFile;
use easyplay_core::error::{ErrorCode, catalog};
use easyplay_core::ids::{QueryId, SessionId};
use easyplay_core::launch::{
    self, HostEnv, InspectorData, InspectorSource, LaunchTarget, PlanRequest,
};
use easyplay_core::model::{
    EndReason, ExeRole, Executable, Fingerprint, Game, GameFlags, LaunchConfig, PeInfo, RuntimeRef,
    Session,
};
use easyplay_core::sandbox;
use easyplay_core::service::{
    DatabaseChoice, Diagnostics, LogChunk, LogFile, RecoveryReason, RecoveryState,
};
use easyplay_testkit::fixtures::hosts::{self, SandboxWorld};
use easyplay_ui::bridge::{InspectTarget, QueryPayload, UiCommand, UiEvent};
use easyplay_ui::input::picker::PickPurpose;
use easyplay_ui::route::{Overlay, Route, SettingsTab};
use easyplay_ui::vm::debug::{InspectorTab, InspectorVm, LogViewerVm, ReportVm};
use egui_kittest::kittest::Queryable;

fn game_of(w: &SandboxWorld) -> Game {
    use easyplay_core::fsx::Fs;
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
        launch: LaunchConfig {
            args: vec!["-windowed".into()],
            ..LaunchConfig::default()
        },
        umu_id: Some("umu-367520".into()),
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

/// A dry run of the worked example, planned by the real planner.
fn sample_inspector() -> InspectorData {
    let w = SandboxWorld::worked_example();
    let g = game_of(&w);
    let env = HostEnv {
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
    };
    let planned = launch::plan_launch(&PlanRequest {
        session_id: hosts::SESSION_ID.parse().unwrap(),
        target: LaunchTarget::Game {
            exe: &g.executables[0],
            extra_args: &[],
        },
        game: Some(&g),
        runtime: Some(&w.runtime),
        proton_codename: None,
        network_override: None,
        host: &w.host,
        env: &env,
    })
    .unwrap();
    let explain = sandbox::explain(&planned.sandbox, &g.policy);
    InspectorData {
        source: InspectorSource::DryRun,
        plan: planned.plan,
        sandbox: Some(planned.sandbox),
        explain,
        timeline: launch::inspect::parse_events(
            "{\"t\":0,\"kind\":\"state\",\"state\":\"validating\"}\n{\"t\":12,\"kind\":\"state\",\"state\":\"planning\"}\n{\"t\":50,\"kind\":\"state\",\"state\":\"spawning\"}\n{\"t\":690,\"kind\":\"phase\",\"phase\":\"launching\"}\n{\"t\":2580,\"kind\":\"state\",\"state\":\"running\"}\n",
        ),
        session: None,
        notes: vec!["PipeWire socket not found".into()],
    }
}

/// At least one node's label contains `text`.
fn has(r: &common::Rig, text: &str) {
    assert!(
        r.h.query_all_by_label_contains(text).next().is_some(),
        "no node contains {text:?}"
    );
}

fn q(n: u128) -> QueryId {
    QueryId::from_uuid(uuid::Uuid::from_u128(n))
}

/// DEV-002: the inspector shows every tab of the real plan; the tabs are reachable and the
/// copy actions work; the Sandbox tab filters its mounts.
#[test]
fn inspector_tabs_and_copy() {
    let mut r = rig(Route::Library);
    let lib = settled_library();
    let g = game_id(&lib, &lib.games[0].name);
    let target = InspectTarget::NextLaunch { game: g, exe: None };
    {
        let s = &mut r.h.state_mut().state;
        s.debug.inspector = Some(InspectorVm::new(target, Some(g), q(1), MonoInstant::ZERO));
        s.nav.open(Overlay::Inspector);
    }
    r.fake.push_event(UiEvent::QueryResult {
        id: q(1),
        payload: QueryPayload::Inspector(Box::new(sample_inspector())),
    });
    r.h.run();
    has(&r, "Hollow Knight");
    has(&r, "Dry run");
    r.h.snapshot("inspector_summary_dark_1280x800");
    // Command: copy as a shell command.
    r.h.get_by_label("Command").click();
    r.h.run();
    r.h.get_by_label("Copy as shell command").click();
    r.h.run();
    has(&r, "Copied");
    // Sandbox: the filter narrows the mounts.
    r.h.get_by_label("Sandbox").click();
    r.h.run();
    has(&r, "System-call filter: on");
    r.h.snapshot("inspector_sandbox_dark_1280x800");
    r.h.get_by_label("Only writable").click();
    r.h.run();
    let vm = r.h.state().state.debug.inspector.clone().unwrap();
    let data = vm.data.ready().unwrap();
    let rows = vm.mounts(data);
    assert!(!rows.is_empty() && rows.iter().all(|m| m.writable()));
    assert!(rows.len() < launch::inspect::mount_rows(data).len());
    for tab in ["Environment", "Access", "Events", "Raw"] {
        r.h.get_by_label(tab).click();
        r.h.run();
    }
    assert_eq!(
        r.h.state().state.debug.inspector.as_ref().unwrap().tab,
        InspectorTab::Raw
    );
    r.h.get_by_label("Copy NUL-separated");
}

fn session(id: u128, open: bool) -> Session {
    let lib = settled_library();
    let mut s = easyplay_testkit::fixtures::games::session(
        &easyplay_testkit::fixtures::games::game(1, 1),
        1,
        1,
        common::now(),
    );
    s.id = SessionId::from_uuid(uuid::Uuid::from_u128(id));
    s.game_id = Some(lib.games[0].id);
    if open {
        s.ended_at = None;
        s.end_reason = None;
        s.state = easyplay_core::model::SessionState::Running;
    } else {
        s.end_reason = Some(EndReason::Crashed { signal: 11 });
    }
    s
}

/// DEV-003: the viewer lists the sessions, loads the log in chunks, renders hostile content
/// as plain text, filters with a match count; *View log* opens it without Developer Mode.
#[test]
fn log_viewer_loads_filters_and_stays_plain() {
    let mut r = rig(Route::Library);
    let lib = settled_library();
    let g = lib.games[0].id;
    let sid = SessionId::from_uuid(uuid::Uuid::from_u128(77));
    {
        let s = &mut r.h.state_mut().state;
        s.debug.logs = Some(LogViewerVm {
            game: Some(g),
            sessions_q: Some(q(2)),
            follow: true,
            ..LogViewerVm::default()
        });
        s.nav.open(Overlay::Logs);
    }
    r.fake.push_event(UiEvent::QueryResult {
        id: q(2),
        payload: QueryPayload::Sessions(vec![session(77, false)]),
    });
    r.h.run();
    let read = r
        .fake
        .sent()
        .into_iter()
        .find_map(|c| match c {
            UiCommand::ReadLog {
                session,
                file,
                offset,
                reply_to,
            } => Some((session, file, offset, reply_to)),
            _ => None,
        })
        .expect("first chunk requested");
    assert_eq!((read.0, read.1, read.2), (sid, LogFile::Game, 0));
    let text = "[+0.100] INFO: umu-launcher version 1.4.4\n[+0.930] Running 'GE-Proton10-25'\n[+2.512] err:module:import_dll Library d3d11.dll not found\n[+2.600] \u{1b}[31m<b>bold</b> [link](http://evil) \u{202e}reversed\n[+2.700] wine: Unhandled page fault\n";
    r.fake.push_event(UiEvent::QueryResult {
        id: read.3,
        payload: QueryPayload::LogChunk(LogChunk {
            session: sid,
            file: LogFile::Game,
            offset: 0,
            bytes: text.as_bytes().to_vec(),
            size: text.len() as u64,
            missing: false,
        }),
    });
    r.h.run();
    has(&r, "5 lines");
    has(&r, "<b>bold</b> [link](http://evil)");
    r.h.snapshot("log_viewer_dark_1280x800");
    let l = r.h.state().state.debug.logs.clone().unwrap();
    assert!(
        l.lines
            .iter()
            .all(|x| !x.contains('\u{1b}') && !x.contains('\u{202e}'))
    );
    has(&r, "2 errors · 0 warnings");
    r.h.get_by_label_contains("The game crashed (signal 11, segmentation fault");
    r.h.state_mut().state.debug.logs.as_mut().unwrap().filter = "ERR:".into();
    r.h.run();
    has(&r, "1 of 5 lines shown");
    // Errors only: Wine's `err:` channel and its crash report, whoever else printed.
    {
        let l = r.h.state_mut().state.debug.logs.as_mut().unwrap();
        l.filter.clear();
        l.level = easyplay_core::diagnostics::log_line::LineLevel::Error;
    }
    r.h.run();
    has(&r, "2 of 5 lines shown");
    // One program: umu's line only.
    {
        let l = r.h.state_mut().state.debug.logs.as_mut().unwrap();
        l.level = easyplay_core::diagnostics::log_line::LineLevel::Debug;
        l.source = Some(easyplay_core::diagnostics::log_line::LineSource::Umu);
    }
    r.h.run();
    has(&r, "1 of 5 lines shown");
}

/// ADR-0025: every game page has a Logs tab, without Developer Mode, and it points to the
/// Proton log for more detail from Wine.
#[test]
fn every_game_page_has_a_logs_tab() {
    let mut r = rig(Route::Library);
    let lib = settled_library();
    let g = lib.games[0].id;
    assert!(!r.h.state().bridge().prefs().advanced.developer_mode);
    r.h.state_mut().state.nav.push(Route::Game(g));
    settle(&mut r);
    r.h.get_by_role_and_label(egui::accesskit::Role::Tab, "Logs")
        .click();
    settle(&mut r);
    has(&r, "turn on “Proton log” in Compatibility");
    // Below the fold at this size: reached by keyboard.
    r.h.get_by_label("Open Compatibility").focus();
    r.h.run();
    r.h.key_press(egui::Key::Enter);
    r.h.run();
    assert_eq!(
        r.h.state().state.details.tab,
        easyplay_ui::vm::details::DetailsTab::Compatibility
    );
}

/// UF-9 / LOGGING §3.3: *View log* on the problem panel opens the in-app viewer for the
/// failed session (not an external program).
#[test]
fn view_log_opens_the_viewer() {
    let mut r = rig(Route::Library);
    let lib = settled_library();
    let g = lib.games[0].id;
    r.h.state_mut().state.nav.push(Route::Game(g));
    settle(&mut r);
    let mut u = catalog::describe(
        &easyplay_core::error::Error::new(ErrorCode::GameCrashed),
        &catalog::DescribeCtx::plain(),
    );
    u.game = Some(g);
    u.session = Some(SessionId::from_uuid(uuid::Uuid::from_u128(5)));
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g,
        state: launch::LaunchState::Failed(Box::new(u)),
    });
    settle(&mut r);
    r.fake.clear_sent();
    r.h.get_by_label("View log").click();
    r.h.run();
    assert!(r.h.state().state.nav.overlays().contains(&Overlay::Logs));
    let sent = r.fake.sent();
    assert!(
        sent.iter()
            .any(|c| matches!(c, UiCommand::ListSessions { game, .. } if *game == g))
    );
    assert!(!sent.iter().any(|c| matches!(c, UiCommand::ViewLog { .. })));
}

/// ERR-003: the preview lists the files with sizes, shows one on demand, and saving asks
/// for a destination.
#[test]
fn report_preview_and_save() {
    let mut r = rig(Route::Library);
    {
        let s = &mut r.h.state_mut().state;
        s.debug.report = Some(ReportVm {
            query: Some(q(3)),
            ..ReportVm::default()
        });
        s.nav.open(Overlay::Report);
    }
    let files = vec![
        ReportFile {
            name: "version.txt".into(),
            bytes: b"EasyPlay 0.1.0\nreport format 1\n".to_vec(),
        },
        ReportFile {
            name: "doctor.json".into(),
            bytes: br#"{ "format": 1, "checks": [ { "id": "D-01", "status": "pass" } ] }"#.to_vec(),
        },
        ReportFile {
            name: "easyplay.log".into(),
            bytes: b"2026-11-12T21:04:11Z INFO start version=0.1.0\n2026-11-12T21:04:12Z WARN library repaired path=~/x\n".to_vec(),
        },
    ];
    r.fake.push_event(UiEvent::QueryResult {
        id: q(3),
        payload: QueryPayload::Report(files),
    });
    r.h.run();
    has(&r, "3 files");
    r.h.get_by_label("View easyplay.log").click();
    r.h.run();
    has(&r, "library repaired path=~/x");
    r.h.snapshot("report_preview_dark_1280x800");
    r.h.get_by_label("Save report…").click();
    r.h.run();
    assert!(r.picker.asked().contains(&PickPurpose::SaveReport));
}

fn crash() -> CrashRecord {
    CrashRecord {
        format: CRASH_FORMAT,
        at: common::now(),
        version: "0.1.0".into(),
        commit: "abc".into(),
        role: "gui".into(),
        thread: "app-service".into(),
        message: "boom".into(),
        location: None,
        backtrace: String::new(),
        uptime_ms: 60_000,
        phase: CrashPhase::Running,
        log_tail: Vec::new(),
    }
}

/// ERR-005 and ERR-002: the crash banner and the Doctor section in Settings → Advanced.
#[test]
fn crash_banner_and_doctor_section() {
    let mut r = rig(Route::Settings(SettingsTab::Advanced));
    r.fake.publish_diagnostics(Diagnostics {
        crash: Some(crash()),
        ..Diagnostics::default()
    });
    settle(&mut r);
    has(&r, "EasyPlay closed unexpectedly");
    has(&r, "System check");
    r.h.snapshot("settings_doctor_crash_dark_1280x800");
    r.h.get_by_label("Check again").click();
    r.h.run();
    assert!(r.fake.sent().contains(&UiCommand::RunDoctor));
    // Create a report from the banner: the preview opens and the banner goes.
    r.fake.clear_sent();
    // The banner's action comes first (the Doctor section has one too).
    r.h.query_all_by_label("Create a report…")
        .next()
        .unwrap()
        .click();
    r.h.run_steps(3);
    let sent = r.fake.sent();
    assert!(sent.contains(&UiCommand::DismissCrash), "{sent:?}");
    assert!(
        sent.iter()
            .any(|c| matches!(c, UiCommand::BuildReport { .. }))
    );
    assert!(r.h.state().state.nav.overlays().contains(&Overlay::Report));
}

/// ERR-004: a damaged library replaces the content with the choices; *Continue* sends the
/// default (the newest backup).
#[test]
fn damaged_library_choices() {
    let mut r = rig(Route::Library);
    let backup = PathBuf::from("/home/sam/.local/share/easyplay/backups/daily-20261111.db");
    let err = catalog::describe(
        &easyplay_core::error::Error::new(ErrorCode::DbCorrupt),
        &catalog::DescribeCtx::plain(),
    );
    r.fake.publish_diagnostics(Diagnostics {
        recovery: Some(RecoveryState {
            reason: RecoveryReason::Damaged {
                backups: vec![(
                    backup.clone(),
                    Timestamp::from_millis(common::now().as_millis() - 86_400_000),
                    12,
                )],
                repaired_games: Some(9),
                missing_games: vec!["Celeste".into(), "Hades".into()],
            },
            error: err,
        }),
        ..Diagnostics::default()
    });
    settle(&mut r);
    has(&r, "The library file is damaged");
    has(&r, "Not recovered: Celeste, Hades");
    r.h.snapshot("recovery_damaged_dark_1280x800");
    r.h.get_by_label("Continue").click();
    r.h.run_steps(2);
    assert!(
        r.fake
            .sent()
            .contains(&UiCommand::ResolveDatabase(DatabaseChoice::RestoreBackup(
                backup
            )))
    );
}

/// DEV-006: with Developer Mode off, a game with developer-only options shows the note and
/// lists them under Review.
#[test]
fn advanced_options_note() {
    let mut r = rig(Route::Library);
    let lib = settled_library();
    let g = lib.games[0].id;
    r.h.state_mut().state.nav.push(Route::Game(g));
    settle(&mut r);
    let rev = r.h.state().bridge().library().revision;
    let mut d = r.h.state().state.details.record().cloned().expect("loaded");
    d.game.launch.env.push(easyplay_core::model::EnvVar {
        name: "LD_PRELOAD".into(),
        value: "/opt/hook.so".into(),
    });
    let a = easyplay_ui::bridge::PolicyAssessment::default();
    r.h.state_mut().state.details.loaded(Box::new(d), a, rev);
    r.h.run();
    r.h.get_by_label("Review").click();
    r.h.run();
    has(&r, "Sets LD_PRELOAD");
}
