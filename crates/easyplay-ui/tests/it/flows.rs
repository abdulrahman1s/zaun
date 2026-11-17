//! Flow tests (UI-015, UI-016, UI-025 … UI-028): setup (UF-2, UF-4) from the file manager
//! and from inside, add game from a folder, prompts and error dialogs with safe defaults,
//! the artwork sheet, quick launch (UF-3); plus their snapshots.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{Rig, game_id, now, rig, rig_app, serve, settle, settled_library};
use easyplay_core::ids::PromptId;
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{ArtSlot, BasePreset, PromptAnswer, PromptKind, PromptRequest};
use easyplay_ui::bridge::{
    AfterCreate, Command, FakeBackend, OpenOrigin, SetupContext, SetupOrigin, UiCommand, UiEvent,
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

/// What the reach lists on screen say ("Allowed: …. Blocked: ….").
fn reach_said(r: &Rig) -> Vec<String> {
    // A text node: kittest reads its words (its value) as its label.
    r.h.query_all_by_label_contains("Allowed:")
        .filter_map(|n| n.accesskit_node().value())
        .filter(|l| l.starts_with("Allowed:"))
        .collect()
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
    let said = reach_said(&r);
    assert!(
        said.iter()
            .any(|l| l.contains("Blocked: Internet and local network")),
        "{said:?}"
    );
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
    let said = reach_said(&r);
    assert!(
        said.iter()
            .any(|l| l.starts_with("Allowed: Internet and local network, its own folder")),
        "{said:?}"
    );
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
    r.h.get_by_label("Programs found in “Celeste”. Pick the one that starts the game.");
    r.h.get_by_role_and_label(Role::RadioButton, "Celeste.exe, main program, 32 MB");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    assert!(r.fake.sent().contains(&UiCommand::Core(Command::OpenPaths {
        paths: vec![folder.join("Celeste.exe")],
        origin: OpenOrigin::Picker,
    })));
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
    assert!(r.fake.sent().contains(&UiCommand::Core(Command::OpenPaths {
        paths: vec![folder.join("Celeste.exe"), folder.join("Config.exe")],
        origin: OpenOrigin::Picker,
    })));
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
            UiCommand::Core(Command::AnswerPrompt { answer, .. }) => Some(answer),
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
    r.h.run_steps(12);
    r.h.get_by_role_and_label(Role::Dialog, "Second problem");
    // The fix is picked by its name (Enter would choose Close, the safe action).
    r.h.get_by_role_and_label(Role::Button, "Edit permissions")
        .click();
    r.h.run_steps(12);
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
    r.h.get_by_role_and_label(Role::Button, "Cover: change. No image yet.")
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

fn artwork_sheet_online(name: &str) -> (Rig, easyplay_core::ids::GameId) {
    let mut r = rig(Route::Library);
    r.h.run();
    let g = game_id(&settled_library(), name);
    let mut prefs = demo::preferences();
    prefs.artwork.online = easyplay_core::config::schema::ArtworkOnline::Steam;
    r.fake.publish_prefs(prefs);
    r.h.state_mut().state.artwork = Some(easyplay_ui::vm::screens::ArtworkVm::new(g, None));
    r.h.state_mut().state.nav.open(Overlay::Artwork(g));
    settle(&mut r);
    (r, g)
}

#[test]
fn artwork_sheet_looks_the_game_up_on_steam_and_downloads_the_chosen_app() {
    let (mut r, g) = artwork_sheet_online("Lantern Keep");
    // Asked once, by the game's name, when the sheet opened.
    let name = "Lantern Keep: The Definitive Edition, Steam app 1547000: download its artwork";
    r.h.get_by_label(name).click();
    r.h.run_steps(2);
    assert!(r.fake.sent().contains(&UiCommand::FetchSteamArtwork {
        id: g,
        app_id: Some(1_547_000),
    }));
    // The download ended (the library changed): the spinners stop.
    r.fake.push_event(UiEvent::LibraryChanged);
    r.h.run_steps(2);
    // Other words are searched for on Search; an app ID or link is a row of its own.
    r.h.state_mut().state.artwork.as_mut().unwrap().term = "nothing at all".into();
    r.h.get_by_label("Search").click();
    settle(&mut r);
    r.h.get_by_label_contains("Nothing found on Steam");
    r.h.state_mut().state.artwork.as_mut().unwrap().term =
        "https://store.steampowered.com/app/12120/".into();
    r.h.run_steps(2);
    r.h.get_by_label("Steam app 12120, Steam app 12120: download its artwork");
}

#[test]
fn a_failed_steam_download_is_said_in_the_sheet() {
    let (mut r, g) = artwork_sheet_online("Lantern Keep");
    r.h.get_by_label("Lantern Keep Trilogy, Steam app 1546990: download its artwork")
        .click();
    r.h.run_steps(2);
    r.fake
        .push_event(UiEvent::Failure(easyplay_core::error::UserError {
            code: easyplay_core::error::ErrorCode::ArtNotFound,
            severity: easyplay_core::error::Severity::Warning,
            title: "No official artwork found".into(),
            message: "Steam has no artwork for this game.".into(),
            actions: vec![],
            details: None,
            game: Some(g),
            session: None,
        }));
    r.h.run_steps(3);
    r.h.get_by_label("Steam has no artwork for this game.");
    assert!(r.h.query_by_role(Role::AlertDialog).is_none());
}

#[test]
fn artwork_sheet_offers_to_turn_online_artwork_on() {
    let mut r = rig(Route::Library);
    r.h.run();
    let g = game_id(&settled_library(), "Lantern Keep");
    r.h.state_mut().state.artwork = Some(easyplay_ui::vm::screens::ArtworkVm::new(g, None));
    r.h.state_mut().state.nav.open(Overlay::Artwork(g));
    r.h.run_steps(4);
    r.h.get_by_label("Turn on").click();
    r.h.run_steps(2);
    assert!(
        r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::SetPreference { .. }))
    );
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
    assert!(r.fake.sent().contains(&UiCommand::Core(Command::OpenPaths {
        paths: vec![path],
        origin: OpenOrigin::FileManager,
    })));
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
            .contains(&UiCommand::Core(Command::CancelLaunch { id: g.id }))
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

fn closes(r: &Rig) -> bool {
    r.h.output()
        .viewport_output
        .values()
        .any(|v| v.commands.contains(&egui::ViewportCommand::Close))
}

/// A game has more programs than its main one; the snapshot lists only that one.
#[test]
fn quick_launch_follows_another_program_in_the_game_folder() {
    let lib = settled_library();
    let g = lib
        .games
        .iter()
        .find(|g| g.root.is_some())
        .expect("a sample game with a folder")
        .clone();
    let other = g.root.as_deref().unwrap().join("bin/Launcher.exe");
    assert_ne!(*other, *g.primary_exe_path);
    let mut r = rig_app(
        FakeBackend::new(lib),
        Route::Library,
        Vec2::new(420.0, 150.0),
        ThemeChoice::Dark,
        move |a| a.follow_quick_launch(other),
    );
    r.h.run_steps(2);
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Running {
            session: easyplay_core::ids::SessionId::from_uuid(uuid::Uuid::from_u128(1)),
        },
    });
    r.h.run_steps(2);
    assert_eq!(r.h.state().state.quick.as_ref().unwrap().game, Some(g.id));
    r.clock.advance(Duration::from_millis(700));
    r.h.step();
    assert!(closes(&r));
}

/// A program that ends at once never shows as running; the window still closes.
#[test]
fn quick_launch_closes_when_the_game_already_ended() {
    let lib = settled_library();
    let g = lib.games[0].clone();
    let path = g.primary_exe_path.to_path_buf();
    let mut r = rig_app(
        FakeBackend::new(lib),
        Route::Library,
        Vec2::new(420.0, 150.0),
        ThemeChoice::Dark,
        move |a| a.follow_quick_launch(path),
    );
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Finished {
            reason: easyplay_core::model::EndReason::Exited { code: 0 },
        },
    });
    r.h.run_steps(2);
    r.clock.advance(Duration::from_millis(700));
    r.h.step();
    assert!(closes(&r));
}

/// Nothing is in progress after *Cancel* (or a launch that stopped to ask a question).
#[test]
fn a_cancelled_quick_launch_shows_no_progress() {
    let lib = settled_library();
    let g = lib.games[0].clone();
    let path = g.primary_exe_path.to_path_buf();
    let mut r = rig_app(
        FakeBackend::new(lib),
        Route::Library,
        Vec2::new(420.0, 150.0),
        ThemeChoice::Dark,
        move |a| a.follow_quick_launch(path),
    );
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Validating,
    });
    r.h.run_steps(3);
    r.h.get_by_role_and_label(Role::ProgressIndicator, "Launch progress");
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Cancelled,
    });
    r.h.run_steps(3);
    r.h.get_by_label("Cancelled");
    assert!(
        r.h.query_by_role_and_label(Role::ProgressIndicator, "Launch progress")
            .is_none()
    );
    assert!(!closes(&r));
}

/// *Play* in the setup sheet shows the launch; a launch that is over without a failure
/// leaves nothing to show, so the sheet closes instead of staying empty.
#[test]
fn the_setup_sheet_closes_when_its_launch_is_cancelled() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.h.run();
    r.h.key_press(Key::Enter);
    r.h.run();
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
        state: LaunchState::Validating,
    });
    r.h.run_steps(3);
    assert!(r.h.state().state.setup.is_some());
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Cancelled,
    });
    r.h.run_steps(3);
    assert!(r.h.state().state.setup.is_none());
    assert!(r.h.state().state.nav.overlays().is_empty());
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
    settle(&mut r);
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

/// Records every saved window geometry.
#[derive(Clone, Default)]
struct SavedGeometry(std::sync::Arc<std::sync::Mutex<Vec<easyplay_ui::WindowGeometry>>>);

impl easyplay_ui::WindowStore for SavedGeometry {
    fn save(&self, geometry: &easyplay_ui::WindowGeometry) {
        self.0.lock().unwrap().push(*geometry);
    }
}

/// The resident service sends the program before the window exists: the compact window only
/// follows it, and its size never becomes the library's saved size.
#[test]
fn resident_quick_launch_follows_without_resending_or_saving_its_size() {
    let fake = FakeBackend::new(settled_library());
    let g = settled_library().games[0].clone();
    let path = g.primary_exe_path.to_path_buf();
    let saved = SavedGeometry::default();
    let s2 = saved.clone();
    let mut r = rig_app(
        fake,
        Route::Library,
        Vec2::new(420.0, 150.0),
        ThemeChoice::Dark,
        move |a| {
            // A saved zoom the compact window does not have: tracking would save a change.
            let initial = easyplay_ui::WindowGeometry {
                ui_scale: 1.5,
                ..easyplay_ui::WindowGeometry::default()
            };
            a.with_window_store(Box::new(s2), initial)
                .follow_quick_launch(path)
        },
    );
    // Past the geometry debounce (1 s of frame time).
    r.h.run_steps(120);
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::Core(Command::OpenPaths { .. }))),
        "{:?}",
        r.fake.sent()
    );
    assert!(saved.0.lock().unwrap().is_empty());
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g.id,
        state: LaunchState::Preparing { phase: None },
    });
    r.h.run_steps(3);
    r.h.get_all_by_label_contains(&g.name).next().unwrap();
}

/// A window that closes while the service runs on releases the flows it was showing.
#[test]
fn closing_the_window_discards_the_setup_and_cancels_prompts() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, EXE, SetupOrigin::FileManager);
    r.fake
        .push_event(UiEvent::Prompt(prompt(PromptKind::InstallerFinished {
            game: game_id(&settled_library(), "Setup Glassworks"),
            programs: Vec::new(),
        })));
    r.h.run();
    r.h.state_mut().abandon_open_flows();
    let sent = r.fake.sent();
    assert!(
        sent.contains(&UiCommand::Core(Command::DiscardDraft)),
        "{sent:?}"
    );
    assert_eq!(answers(&r), vec![PromptAnswer::Cancel]);
}

/// Add a game is one dialog in three steps that morph into each other: choose the folder
/// (the picker scans it; the dialog stays up between the steps), pick the program, set it
/// up. *Back* returns to the programs, and the poster follows the name as it is typed.
#[test]
fn add_a_game_takes_three_steps_in_one_dialog() {
    let mut r = rig(Route::Library);
    r.h.run();
    easyplay_ui::screens::add_game::open(&mut r.h.state_mut().state);
    r.h.run_steps(4);
    r.h.get_by_role_and_label(Role::Dialog, "Add a game");
    let folder = PathBuf::from("/home/sam/Games/Celeste");
    r.picker.answer(vec![folder.clone()]);
    r.h.get_by_role_and_label(Role::Button, "Choose folder…")
        .click();
    // The folder is asked for with a folder picker, and scanned; the dialog never closes
    // on the way to the second step.
    for _ in 0..6 {
        r.h.step();
        assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
        assert!(r.h.state().state.add_game.is_some());
    }
    assert_eq!(
        r.picker.asked(),
        vec![easyplay_ui::input::picker::PickPurpose::AddGameFolder]
    );
    let q = r
        .fake
        .sent()
        .into_iter()
        .find_map(|c| match c {
            UiCommand::Core(Command::ScanFolder {
                folder: f,
                reply_to,
            }) if f == folder => Some(reply_to),
            _ => None,
        })
        .expect("the folder is scanned");
    r.fake.push_event(UiEvent::QueryResult {
        id: q,
        payload: easyplay_ui::bridge::QueryPayload::Folder(demo::folder_scan(&folder)),
    });
    r.h.run_steps(4);
    r.h.get_by_label("Programs found in “Celeste”. Pick the one that starts the game.");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run_steps(2);
    assert!(r.fake.sent().contains(&UiCommand::Core(Command::OpenPaths {
        paths: vec![folder.join("Celeste.exe")],
        origin: OpenOrigin::Picker,
    })));
    // The service asks for the setup: the third step, in the same dialog.
    request_setup(&r, EXE, SetupOrigin::InsideApp);
    r.h.run_steps(6);
    r.h.get_by_role_and_label(Role::Dialog, "Set up this game");
    r.h.get_by_role_and_label(Role::Image, "Cover preview: Celeste");
    let name = r.h.get_by_role_and_label(Role::TextInput, "Name");
    name.focus();
    r.h.run();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    r.h.event(egui::Event::Text("Celeste Classic".into()));
    r.h.run_steps(2);
    r.h.get_by_role_and_label(Role::Image, "Cover preview: Celeste Classic");
    // Back: the programs of the folder again, and the draft is let go.
    r.fake.clear_sent();
    r.h.get_by_role_and_label(Role::Button, "Back").click();
    r.h.run_steps(8);
    assert!(
        r.fake
            .sent()
            .contains(&UiCommand::Core(Command::DiscardDraft))
    );
    assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
    r.h.get_by_role_and_label(Role::RadioButton, "Celeste.exe, main program, 32 MB");
}

/// Toasts show one at a time: a new one takes the place of the one showing, and goes after
/// its time.
#[test]
fn toasts_show_one_at_a_time() {
    use easyplay_ui::vm::toasts::ToastKind;
    let mut r = rig(Route::Library);
    r.h.run();
    let mono = easyplay_core::clock::Clock::mono(&*r.clock);
    let s = &mut r.h.state_mut().state;
    s.toast("First toast", ToastKind::Info, None, mono);
    s.toast("Second toast", ToastKind::Info, None, mono);
    r.h.run_steps(4);
    // (The toast, and what is announced of it.)
    r.h.get_all_by_label("Second toast").next().unwrap();
    assert!(r.h.query_all_by_label("First toast").next().is_none());
    r.clock.advance(Duration::from_secs(4));
    r.h.run_steps(20);
    assert!(
        r.h.query_all_by_role_and_label(Role::Status, "Second toast")
            .all(|n| n.accesskit_node().bounding_box().is_none())
    );
}

/// Hiding several games offers one *Undo*, which brings every one of them back.
#[test]
fn undo_of_hiding_several_games_brings_all_of_them_back() {
    let lib = settled_library();
    let ids: Vec<_> = ["Lantern Keep", "Amber Sky", "Hollow Bell"]
        .iter()
        .map(|n| game_id(&lib, n))
        .collect();
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.state_mut().state.library.selection = ids.clone();
    r.h.run_steps(4);
    // The filter line has become the selection's actions.
    r.h.get_by_role_and_label(Role::Button, "Hide").click();
    r.h.run_steps(4);
    for id in &ids {
        assert!(r.fake.sent().contains(&UiCommand::Core(Command::SetHidden {
            id: *id,
            hidden: true
        })));
    }
    r.h.get_all_by_label("Hid 3 games").next().unwrap();
    r.fake.clear_sent();
    r.h.get_by_role_and_label(Role::Button, "Undo").click();
    r.h.run_steps(4);
    let sent = r.fake.sent();
    for id in &ids {
        assert!(
            sent.contains(&UiCommand::Core(Command::SetHidden {
                id: *id,
                hidden: false
            })),
            "{id:?} comes back"
        );
    }
}

/// Remove asks with four switches; keeping the saves means something only while the
/// private storage is deleted, and the choices go with the removal.
#[test]
fn the_remove_dialog_has_four_switches() {
    let mut r = rig(Route::Library);
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
    r.h.get_by_role_and_label(Role::Dialog, "Remove Lantern Keep?");
    let switch = |r: &Rig, l: &str| {
        let n = r.h.get_by_role_and_label(Role::Switch, l);
        let n = n.accesskit_node();
        (
            n.toggled() == Some(egui::accesskit::Toggled::True),
            n.is_disabled(),
        )
    };
    assert_eq!(switch(&r, "Delete private Windows storage"), (true, false));
    assert_eq!(switch(&r, "Keep save data"), (true, false));
    assert_eq!(switch(&r, "Remove menu shortcuts"), (true, false));
    assert_eq!(switch(&r, "Keep play history"), (true, false));
    r.h.get_by_role_and_label(Role::Switch, "Delete private Windows storage")
        .click();
    r.h.run();
    assert!(switch(&r, "Keep save data").1, "nothing to keep");
    r.h.get_by_role_and_label(Role::Switch, "Keep play history")
        .click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Remove game")
        .click();
    r.h.run_steps(4);
    let options = r
        .fake
        .sent()
        .into_iter()
        .find_map(|c| match c {
            UiCommand::Core(Command::RemoveGame { id, options }) if id == g => Some(options),
            _ => None,
        })
        .expect("the game is removed");
    assert!(!options.delete_private_storage);
    assert!(!options.keep_history);
    assert!(options.delete_shortcuts);
}

/// Play from the wall on a game whose last start failed shows that failure (its dialog
/// offers *Try again*) instead of failing again; a game whose last session ended at once
/// opens its page, where the note about it is.
#[test]
fn play_from_the_wall_with_an_open_problem_shows_the_problem() {
    let lib = settled_library();
    let g = game_id(&lib, "Lantern Keep");
    let mut r = rig(Route::Library);
    r.h.run();
    r.fake.push_event(UiEvent::LaunchProgress {
        game: g,
        state: LaunchState::Failed(Box::new(demo::launch_failure(g, "Lantern Keep"))),
    });
    r.h.run_steps(8);
    assert!(r.h.state().state.launches.failures.contains_key(&g));
    assert!(r.h.state().state.nav.overlays().is_empty());
    r.fake.clear_sent();
    r.h.get_by_role_and_label(Role::Button, "Play Lantern Keep")
        .click();
    r.h.run_steps(8);
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::Launch { .. })),
        "not started again"
    );
    r.h.get_all_by_role_and_label(Role::Dialog, "This game needs a folder that isn't there")
        .next()
        .unwrap();
    // A session that ended at once: the page, with its note.
    let h = game_id(&lib, "Amber Sky");
    r.h.state_mut()
        .state
        .launches
        .ended_early
        .insert(h, easyplay_core::model::EndReason::Crashed { signal: 11 });
    r.h.key_press(Key::Escape);
    r.h.run_steps(12);
    assert!(r.h.state().state.nav.overlays().is_empty());
    r.fake.clear_sent();
    r.h.get_by_role_and_label(Role::Button, "Play Amber Sky")
        .click();
    r.h.run_steps(8);
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::Launch { .. }))
    );
    assert_eq!(r.h.state().state.nav.current(), Route::Game(h));
}

/// *Choose a file instead…* in the second step: the dialog stays while the file is set
/// up, and the third step offers *Back* to the programs.
#[test]
fn choose_a_file_instead_keeps_the_dialog_and_back_returns() {
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
    r.h.run_steps(6);
    r.picker.answer(vec![PathBuf::from(EXE)]);
    r.h.get_by_role_and_label(Role::Button, "Choose a file instead…")
        .click();
    for _ in 0..6 {
        r.h.step();
        assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
    }
    assert!(r.fake.sent().contains(&UiCommand::Core(Command::OpenPaths {
        paths: vec![PathBuf::from(EXE)],
        origin: OpenOrigin::Picker,
    })));
    request_setup(&r, EXE, SetupOrigin::InsideApp);
    r.h.run_steps(6);
    r.h.get_by_role_and_label(Role::Dialog, "Set up this game");
    r.h.get_by_role_and_label(Role::Button, "Back").click();
    r.h.run_steps(8);
    assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
    r.h.get_by_role_and_label(Role::RadioButton, "Celeste.exe, main program, 32 MB");
}
