//! Flow tests (UI-015, UI-016, UI-025 … UI-028): setup (UF-2, UF-4) from the file manager
//! and from inside, add game from a folder, prompts and error dialogs with safe defaults,
//! the artwork sheet, quick launch (UF-3); plus their snapshots.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{Rig, game_id, now, rig, rig_app, serve, settle, settled_library};
use easyplay_core::ids::PromptId;
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{ArtSlot, BasePreset, PromptAnswer, PromptKind, PromptRequest};
use easyplay_ui::bridge::{
    AfterCreate, FakeBackend, OpenOrigin, SetupContext, SetupOrigin, UiCommand, UiEvent,
};
use easyplay_ui::demo;
use easyplay_ui::route::{Overlay, Route};
use easyplay_ui::theme::ThemeChoice;
use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};

const EXE: &str = "/home/sam/Games/Celeste/Celeste.exe";

fn request_setup(r: &Rig, path: &str, origin: SetupOrigin) {
    r.fake.push_event(UiEvent::SetupRequested {
        draft: Box::new(demo::draft(Path::new(path), now())),
        context: SetupContext {
            origin,
            queue: (1, 1),
        },
    });
}

fn created(r: &Rig) -> Option<(String, AfterCreate)> {
    r.fake.sent().into_iter().find_map(|c| match c {
        UiCommand::CreateGame { draft, then } => Some((draft.name, then)),
        _ => None,
    })
}

#[test]
fn uf2_enter_plays_from_the_file_manager_and_shows_progress() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Set up this game");
    r.h.get_by_label_contains("No internet or local network");
    r.h.key_press(Key::Enter);
    r.h.run();
    assert_eq!(created(&r), Some(("Celeste".into(), AfterCreate::Launch)));
    // The service adds the game and starts it.
    let lib = r.h.state().bridge().library().clone();
    let mut games = lib.games.clone();
    let mut g = games[0].clone();
    g.id = easyplay_core::ids::GameId::from_uuid(uuid::Uuid::from_u128(0xCE1E));
    g.name = "Celeste".into();
    g.primary_exe_path = std::sync::Arc::from(Path::new(EXE));
    games.push(g.clone());
    r.fake.publish_library(easyplay_ui::bridge::library_from(
        games,
        demo::sample_tags(),
        1,
        now(),
    ));
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Preparing { phase: None },
    });
    r.h.run_steps(3);
    r.h.get_all_by_label("Preparing private Windows storage…")
        .next()
        .unwrap();
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Running {
            session: easyplay_core::ids::SessionId::from_uuid(uuid::Uuid::from_u128(1)),
        },
    });
    r.h.run_steps(3);
    assert!(r.h.state().state.setup.is_none());
}

#[test]
fn uf4_inside_easyplay_the_primary_adds_to_the_library() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::InsideApp);
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Add to library")
        .click();
    r.h.run();
    assert_eq!(created(&r), Some(("Celeste".into(), AfterCreate::Nothing)));
    assert!(r.h.state().state.nav.overlays().is_empty());
}

#[test]
fn alt_digits_switch_presets_and_the_summary_follows() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.h.run();
    r.h.key_press_modifiers(Modifiers::ALT, Key::Num2);
    r.h.run();
    r.h.get_by_label_contains("Can use the internet and your local network");
    let draft = &r.h.state().state.setup.as_ref().unwrap().draft;
    assert_eq!(draft.policy.preset.base(), BasePreset::Online);
}

#[test]
fn an_empty_name_disables_play_with_a_reason() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.h.run();
    r.h.state_mut().state.setup.as_mut().unwrap().name.clear();
    r.h.run();
    let play = r.h.get_by_role_and_label(Role::Button, "Play");
    assert!(play.accesskit_node().is_disabled());
    r.h.get_by_label("Fix the marked problem to continue");
}

#[test]
fn loose_files_offer_this_file_only_and_installers_run() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(
        &r,
        "/home/sam/Downloads/setup_glassworks.exe",
        SetupOrigin::FileManager,
    );
    r.h.run();
    r.h.get_by_label_contains("This program is not in a folder of its own");
    r.h.get_by_label_contains("This looks like an installer");
    r.h.get_by_role_and_label(Role::Button, "Run installer");
}

#[test]
fn escape_cancels_setup_without_saving() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.h.run();
    r.h.key_press(Key::Escape);
    r.h.run();
    assert!(created(&r).is_none());
    assert!(r.h.state().state.nav.overlays().is_empty());
}

#[test]
fn a_dropped_folder_lists_programs_and_continue_opens_the_choice() {
    let mut r = rig(Route::Library);
    r.h.run();
    let folder = PathBuf::from("/home/sam/Games/Celeste");
    r.h.state_mut().state.add_game = Some(easyplay_ui::vm::screens::AddGameVm {
        folder: folder.clone(),
        ..Default::default()
    });
    let q = r.h.state().bridge().query_id();
    r.h.state_mut().state.add_game.as_mut().unwrap().query = Some(q);
    r.fake.push_event(UiEvent::QueryResult {
        id: q,
        payload: easyplay_ui::bridge::QueryPayload::Folder(demo::folder_scan(&folder)),
    });
    r.h.run();
    assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
    r.h.get_by_label("Programs found in “Celeste”");
    r.h.get_by_role_and_label(Role::RadioButton, "Celeste.exe, main program, 32 MB");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    assert!(r.fake.sent().contains(&UiCommand::OpenPaths {
        paths: vec![folder.join("Celeste.exe")],
        origin: OpenOrigin::Picker,
    }));
}

#[test]
fn add_several_switches_to_checkboxes() {
    let mut r = rig(Route::Library);
    r.h.run();
    let folder = PathBuf::from("/home/sam/Games/Celeste");
    let mut vm = easyplay_ui::vm::screens::AddGameVm {
        folder: folder.clone(),
        ..Default::default()
    };
    vm.set_scan(demo::folder_scan(&folder));
    r.h.state_mut().state.add_game = Some(vm);
    r.h.state_mut().state.nav.open(Overlay::AddGame);
    r.h.run();
    r.h.get_by_label("Add several").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::CheckBox, "Config.exe, looks like a tool, 1 MB")
        .click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    assert!(r.fake.sent().contains(&UiCommand::OpenPaths {
        paths: vec![folder.join("Celeste.exe"), folder.join("Config.exe")],
        origin: OpenOrigin::Picker,
    }));
}

fn prompt(kind: PromptKind) -> PromptRequest {
    PromptRequest {
        id: PromptId::from_uuid(uuid::Uuid::from_u128(77)),
        kind,
    }
}

fn answers(r: &Rig) -> Vec<PromptAnswer> {
    r.fake
        .sent()
        .into_iter()
        .filter_map(|c| match c {
            UiCommand::AnswerPrompt { answer, .. } => Some(answer),
            _ => None,
        })
        .collect()
}

/// Every prompt dialog: `Enter` picks the safe default, `Esc` cancels; both are accepted.
#[test]
fn every_prompt_defaults_to_the_safe_choice() {
    let lib = settled_library();
    let g = game_id(&lib, "Lantern Keep");
    let fp = easyplay_core::model::Fingerprint {
        size: 1,
        mtime_ns: 0,
        quick_hash: [0; 32],
        full_hash: None,
    };
    let kinds = vec![
        PromptKind::ExeChanged {
            game: g,
            exe: easyplay_core::ids::ExecutableId::from_uuid(uuid::Uuid::from_u128(1)),
            old: fp,
            new: fp,
        },
        PromptKind::InsideKnownRoot {
            game: g,
            path: "/g/Config.exe".into(),
        },
        PromptKind::CopyOfKnown {
            game: g,
            path: "/g/copy.exe".into(),
        },
        PromptKind::GrantTargetChanged {
            game: g,
            grant: "/home/sam/Mods".into(),
            old: "/home/sam/Mods".into(),
            new: "/mnt/data/Mods".into(),
        },
        PromptKind::RuntimeUnavailable {
            game: g,
            reference: easyplay_core::model::RuntimeRef::Default,
        },
        PromptKind::FixNeedsNetwork {
            game: g,
            prefix_tainted: true,
        },
        PromptKind::ToolNeedsNetwork {
            game: g,
            tool: easyplay_core::model::Tool::Winecfg,
            prefix_tainted: false,
        },
        PromptKind::InstallerFinished {
            game: g,
            programs: vec![easyplay_core::model::ProgramCandidate {
                path: "/p/Game.exe".into(),
                label: "Game".into(),
                from_shortcut: true,
                size: 3_000_000,
            }],
        },
        PromptKind::ConfirmBroadGrant {
            game: Some(g),
            path: "/home/sam/Documents".into(),
            reason: "user_dir".into(),
        },
        PromptKind::RemoveRunningGame { game: g },
    ];
    for kind in kinds {
        for key in [Key::Enter, Key::Escape] {
            let mut r = common::rig_with(
                lib.clone(),
                Route::Library,
                Vec2::new(1280.0, 800.0),
                ThemeChoice::Dark,
            );
            r.h.run();
            r.fake.push_event(UiEvent::Prompt(prompt(kind.clone())));
            r.h.run();
            r.h.get_all_by_role(Role::Dialog).next().unwrap();
            r.h.key_press(key);
            r.h.run();
            let a = answers(&r);
            assert_eq!(a.len(), 1, "{kind:?} {key:?}");
            assert!(kind.accepts(&a[0]), "{kind:?}: {:?}", a[0]);
            let safe = matches!(
                a[0],
                PromptAnswer::Cancel | PromptAnswer::NotNow | PromptAnswer::KeepAsInstaller
            );
            assert!(safe, "{kind:?} {key:?}: {:?} is not the safe choice", a[0]);
        }
    }
}

#[test]
fn errors_queue_and_say_how_many_more() {
    let mut r = rig(Route::Library);
    r.h.run();
    let g = game_id(&settled_library(), "Lantern Keep");
    let mut a = demo::launch_failure(g, "Lantern Keep");
    a.title = "First problem".into();
    let mut b = a.clone();
    b.title = "Second problem".into();
    r.fake.push_event(UiEvent::Failure(a));
    r.fake.push_event(UiEvent::Failure(b));
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "First problem");
    r.h.get_by_label("1 more problem");
    r.h.key_press(Key::Escape);
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Second problem");
    // Enter = the primary recovery action.
    r.h.key_press(Key::Enter);
    r.h.run();
    assert_eq!(r.h.state().state.nav.current(), Route::Game(g));
}

#[test]
fn artwork_sheet_shows_import_errors_inline_and_keeps_the_old_art() {
    let mut r = rig(Route::Library);
    r.h.run();
    let g = game_id(&settled_library(), "Lantern Keep");
    r.h.state_mut().state.artwork = Some(easyplay_ui::vm::screens::ArtworkVm::new(g, None));
    r.h.state_mut().state.nav.open(Overlay::Artwork(g));
    r.picker.answer(vec!["/home/sam/Pictures/huge.png".into()]);
    r.h.run();
    r.h.get_all_by_label("Choose image…")
        .next()
        .unwrap()
        .click();
    r.h.run_steps(3);
    assert!(r.fake.sent().iter().any(|c| matches!(
        c,
        UiCommand::ImportArtwork {
            slot: ArtSlot::Cover,
            ..
        }
    )));
    r.fake.push_event(UiEvent::ArtworkFailed {
        game: g,
        slot: ArtSlot::Cover,
        error: easyplay_core::error::UserError {
            code: easyplay_core::error::ErrorCode::ArtTooLarge,
            severity: easyplay_core::error::Severity::Error,
            title: "This image is too large".into(),
            message: "This image is too large (max 64 megapixels).".into(),
            actions: vec![],
            details: None,
            game: Some(g),
            session: None,
        },
    });
    r.h.run_steps(3);
    r.h.get_by_label("This image is too large (max 64 megapixels).");
}

#[test]
fn quick_launch_opens_the_program_and_closes_after_running() {
    let fake = FakeBackend::new(settled_library());
    let lib = settled_library();
    let g = lib
        .games
        .iter()
        .find(|g| &*g.name == "Lantern Keep")
        .unwrap()
        .clone();
    let path = g.primary_exe_path.to_path_buf();
    let p2 = path.clone();
    let mut r = rig_app(
        fake,
        Route::Library,
        Vec2::new(420.0, 150.0),
        ThemeChoice::Dark,
        move |a| a.quick_launch(p2),
    );
    r.h.run_steps(2);
    assert!(r.fake.sent().contains(&UiCommand::OpenPaths {
        paths: vec![path],
        origin: OpenOrigin::FileManager,
    }));
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Preparing { phase: None },
    });
    r.h.run_steps(3);
    r.h.get_all_by_label_contains("Lantern Keep")
        .next()
        .unwrap();
    r.h.get_by_role_and_label(Role::Button, "Cancel").click();
    r.h.run_steps(2);
    assert!(
        r.fake
            .sent()
            .contains(&UiCommand::CancelLaunch { id: g.id })
    );
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Running {
            session: easyplay_core::ids::SessionId::from_uuid(uuid::Uuid::from_u128(1)),
        },
    });
    r.h.run_steps(2);
    r.clock.advance(Duration::from_millis(700));
    r.h.step();
    let closed =
        r.h.output()
            .viewport_output
            .values()
            .any(|v| v.commands.contains(&egui::ViewportCommand::Close));
    assert!(closed);
}

// ---- Snapshots ---------------------------------------------------------------------------

#[test]
fn snapshot_setup_dark() {
    let mut r = common::rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.h.run_steps(6);
    r.h.snapshot("setup_dark_1280x900");
}

#[test]
fn snapshot_setup_light_more_options() {
    let mut r = common::rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Light,
    );
    r.h.run();
    request_setup(
        &r,
        "/home/sam/Downloads/OnlineArena.exe",
        SetupOrigin::InsideApp,
    );
    r.h.run();
    r.h.state_mut().state.setup.as_mut().unwrap().more = true;
    r.h.run_steps(6);
    r.h.snapshot("setup_light_1280x900");
}

#[test]
fn snapshot_setup_compact() {
    let mut r = common::rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.h.run_steps(6);
    r.h.snapshot("setup_dark_640x480");
}

#[test]
fn snapshot_add_game_dark() {
    let mut r = rig(Route::Library);
    r.h.run();
    let folder = PathBuf::from("/home/sam/Games/Celeste");
    let mut vm = easyplay_ui::vm::screens::AddGameVm {
        folder: folder.clone(),
        show_filtered: true,
        ..Default::default()
    };
    vm.set_scan(demo::folder_scan(&folder));
    r.h.state_mut().state.add_game = Some(vm);
    r.h.state_mut().state.nav.open(Overlay::AddGame);
    r.h.run_steps(6);
    r.h.snapshot("add_game_dark_1280x800");
}

#[test]
fn snapshot_dialog_remove_light() {
    let mut r = common::rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Light,
    );
    r.h.run();
    let g = game_id(&settled_library(), "Lantern Keep");
    r.h.state_mut()
        .state
        .show_dialog(easyplay_ui::vm::dialogs::Dialog::Confirm(
            easyplay_ui::vm::dialogs::Confirm::RemoveGames {
                ids: vec![g],
                options: Default::default(),
            },
        ));
    r.h.run_steps(6);
    r.h.snapshot("dialog_remove_light_1280x800");
}

#[test]
fn snapshot_error_dialog_dark() {
    let mut r = rig(Route::Library);
    r.h.run();
    let g = game_id(&settled_library(), "Lantern Keep");
    r.fake
        .push_event(UiEvent::Failure(demo::launch_failure(g, "Lantern Keep")));
    r.fake
        .push_event(UiEvent::Failure(demo::launch_failure(g, "Moss & Iron")));
    r.h.run();
    r.h.get_by_label("Details").click();
    r.h.run_steps(6);
    r.h.snapshot("dialog_error_dark_1280x800");
}

#[test]
fn snapshot_prompt_installer_dark() {
    let mut r = rig(Route::Library);
    r.h.run();
    let g = game_id(&settled_library(), "Setup Glassworks");
    r.fake
        .push_event(UiEvent::Prompt(prompt(PromptKind::InstallerFinished {
            game: g,
            programs: vec![
                easyplay_core::model::ProgramCandidate {
                    path: "/p/Glassworks.exe".into(),
                    label: "Glassworks".into(),
                    from_shortcut: true,
                    size: 48_000_000,
                },
                easyplay_core::model::ProgramCandidate {
                    path: "/p/Launcher.exe".into(),
                    label: "Glassworks Launcher".into(),
                    from_shortcut: false,
                    size: 2_000_000,
                },
            ],
        })));
    r.h.run_steps(6);
    r.h.snapshot("prompt_installer_dark_1280x800");
}

#[test]
fn snapshot_artwork_sheet_dark() {
    let mut r = rig(Route::Library);
    r.h.run();
    let g = game_id(&settled_library(), "Lantern Keep");
    let mut prefs = demo::preferences();
    prefs.artwork.online = easyplay_core::config::schema::ArtworkOnline::Steam;
    r.fake.publish_prefs(prefs);
    r.h.state_mut().state.artwork = Some(easyplay_ui::vm::screens::ArtworkVm::new(g, None));
    r.h.state_mut().state.nav.open(Overlay::Artwork(g));
    r.h.run_steps(6);
    r.h.snapshot("artwork_dark_1280x800");
}

#[test]
fn snapshot_shortcut_sheet_light() {
    let mut r = common::rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Light,
    );
    r.h.run();
    r.h.key_press(Key::F1);
    r.h.run_steps(6);
    r.h.snapshot("shortcuts_light_1280x900");
}

#[test]
fn snapshot_quick_launch() {
    let fake = FakeBackend::new(settled_library());
    let lib = settled_library();
    let g = lib
        .games
        .iter()
        .find(|g| &*g.name == "Lantern Keep")
        .unwrap()
        .clone();
    let path = g.primary_exe_path.to_path_buf();
    let mut r = rig_app(
        fake,
        Route::Library,
        Vec2::new(420.0, 150.0),
        ThemeChoice::Dark,
        move |a| a.quick_launch(path),
    );
    r.h.run_steps(2);
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Validating,
    });
    r.h.run_steps(4);
    r.h.snapshot("quick_launch_dark_420x150");
    let _ = (serve, settle);
}
