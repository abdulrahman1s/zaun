//! Shell and library interaction tests (UI-001, UI-008 … UI-013, UI-030, UI-031) over a fake
//! backend, plus the snapshot images of TESTING_STRATEGY §9 for the shell and library.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{game_id, now, rig, rig_with, settled_library};
use easyplay_core::library::SortKey;
use easyplay_ui::bridge::{FakeBackend, LibraryView, OpenOrigin, UiBridge, UiCommand};
use easyplay_ui::input::picker::PickPurpose;
use easyplay_ui::route::{Overlay, Route};
use easyplay_ui::theme::{self, Appearance, ThemeChoice};
use easyplay_ui::vm::library::{Collection, ViewMode};
use easyplay_ui::{EasyPlayApp, demo};
use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

#[test]
fn boots_to_the_library_with_collection_counts() {
    let mut r = rig(Route::Library);
    r.h.run();
    let all = demo::library(now()).games.len();
    r.h.get_by_label(&format!("All games, {all}"));
    r.h.get_by_role(Role::Navigation);
    r.h.get_by_label_contains("Games, 24 items");
    r.h.get_by_label("Search games");
    r.h.get_by_role_and_label(Role::Button, "Add game");
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn sidebar_click_selects_a_collection() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Favourites, ").click();
    r.h.run();
    assert_eq!(r.h.state().state.library.collection, Collection::Favorites);
    r.h.get_by_label_contains("Games, 3 items");
}

#[test]
fn sidebar_arrow_keys_move_focus_and_enter_selects() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("All games, ").focus();
    r.h.run();
    r.h.key_press(Key::ArrowDown);
    r.h.run();
    assert!(r.h.get_by_label_contains("Favourites, ").is_focused());
    r.h.key_press(Key::Enter);
    r.h.run();
    assert_eq!(r.h.state().state.library.collection, Collection::Favorites);
}

#[test]
fn ctrl_b_collapses_the_sidebar_to_a_rail() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label("Collapse sidebar");
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    r.h.run();
    r.h.get_by_label("Expand sidebar");
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    r.h.run();
    r.h.get_by_label("Collapse sidebar");
}

#[test]
fn small_window_collapses_the_sidebar_automatically() {
    let mut r = rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    r.h.get_by_label("Expand sidebar");
    r.h.get_by_label_contains("Games, 24 items");
}

#[test]
fn clicking_a_card_opens_the_game_page_and_back_returns() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.").click();
    r.h.run();
    assert!(matches!(r.h.state().state.nav.current(), Route::Game(_)));
    r.h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    r.h.run();
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn keyboard_plays_and_favourites_the_focused_card() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.").focus();
    r.h.run();
    r.h.key_press(Key::Space);
    r.h.run();
    r.h.key_press(Key::F);
    r.h.run();
    let id = game_id(&demo::library(now()), "Lantern Keep");
    assert_eq!(
        r.fake.sent(),
        vec![
            UiCommand::Launch { id, exe: None },
            UiCommand::SetFavorite {
                id,
                favorite: false
            }
        ]
    );
    r.h.get_by_role_and_label(Role::Status, "Removed Lantern Keep from favourites");
}

#[test]
fn undo_reverses_the_last_change() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.").focus();
    r.h.run();
    r.h.key_press(Key::F);
    r.h.run();
    r.fake.clear_sent();
    r.h.get_by_label("Undo").click();
    r.h.run();
    let id = game_id(&demo::library(now()), "Lantern Keep");
    assert_eq!(
        r.fake.sent(),
        vec![UiCommand::SetFavorite { id, favorite: true }]
    );
}

#[test]
fn arrow_keys_move_through_the_grid() {
    let mut r = rig(Route::Library);
    r.h.run();
    let lib = demo::library(now());
    let first = &lib.games[0];
    r.h.get_by_label_contains(&format!("{}. ", first.name))
        .focus();
    r.h.run();
    r.h.key_press(Key::ArrowRight);
    r.h.run();
    assert_eq!(r.h.state().state.library.selected, Some(lib.games[1].id));
    r.h.key_press(Key::ArrowLeft);
    r.h.run();
    assert_eq!(r.h.state().state.library.selected, Some(first.id));
}

#[test]
fn search_with_no_results_offers_to_clear() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label("Search games").focus();
    r.h.get_by_label("Search games").type_text("zzz");
    r.h.run();
    r.h.get_by_label("No games match “zzz”.");
    r.h.get_all_by_role_and_label(Role::Button, "Clear search")
        .last()
        .unwrap()
        .click();
    r.h.run();
    assert!(r.h.state().state.library.query.is_empty());
}

#[test]
fn search_operators_filter_in_the_same_frame() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label("Search games").focus();
    r.h.get_by_label("Search games").type_text("is:favorite");
    r.h.run();
    r.h.get_by_label_contains("Games, 3 items");
    r.h.get_all_by_label_contains("3 of 24").next().unwrap();
}

#[test]
fn type_to_search_starts_from_anywhere_in_the_library() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.event(egui::Event::Text("moss".into()));
    r.h.run();
    assert_eq!(r.h.state().state.library.query, "moss");
    r.h.get_by_label_contains("Games, 1 item");
}

#[test]
fn filter_chips_narrow_and_clear() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label("Filter").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::CheckBox, "Online").click();
    r.h.run();
    r.h.key_press(Key::Escape);
    r.h.run();
    let n = r.h.state().state.library.chips().len();
    assert_eq!(n, 1);
    r.h.get_by_label("Remove Online").click();
    r.h.run();
    assert!(r.h.state().state.library.chips().is_empty());
}

#[test]
fn list_view_sorts_by_header_and_flips() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    r.h.run();
    assert_eq!(r.h.state().state.library.view, ViewMode::List);
    r.h.get_by_role_and_label(Role::ColumnHeader, "Playtime")
        .click();
    r.h.run();
    assert_eq!(r.h.state().state.library.sort, SortKey::Playtime);
    assert!(r.h.state().state.library.descending);
    r.h.get_by_role_and_label(Role::ColumnHeader, "Playtime ↓")
        .click();
    r.h.run();
    assert!(!r.h.state().state.library.descending);
}

#[test]
fn ctrl_click_and_shift_click_build_a_selection_with_a_bar() {
    let mut r = rig(Route::Library);
    r.h.run();
    let lib = demo::library(now());
    r.h.get_by_label_contains(&format!("{}. ", lib.games[0].name))
        .click_modifiers(Modifiers::COMMAND);
    r.h.run();
    r.h.get_by_label_contains(&format!("{}. ", lib.games[3].name))
        .click_modifiers(Modifiers::SHIFT);
    r.h.run();
    assert_eq!(r.h.state().state.library.selection.len(), 4);
    r.h.get_by_label("4 selected");
    r.h.get_by_label("Favourite").click();
    r.h.run();
    let favs = r
        .fake
        .sent()
        .iter()
        .filter(|c| matches!(c, UiCommand::SetFavorite { favorite: true, .. }))
        .count();
    assert_eq!(favs, 4);
    r.h.key_press(Key::Escape);
    r.h.run();
    assert!(r.h.state().state.library.selection.is_empty());
}

#[test]
fn bulk_remove_asks_once_with_cancel_as_default() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    r.h.run();
    r.h.get_by_label("Remove…").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Remove 24 games from EasyPlay?");
    // Enter activates the safe default.
    r.h.key_press(Key::Enter);
    r.h.run();
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::RemoveGame { .. }))
    );
    assert!(r.h.state().state.nav.overlays().is_empty());
}

#[test]
fn context_menu_opens_by_keyboard_and_removes_with_confirmation() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.").focus();
    r.h.run();
    r.h.key_press_modifiers(Modifiers::SHIFT, Key::F10);
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, "Open game page");
    r.h.get_by_role_and_label(Role::MenuItem, "Remove…").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Remove Lantern Keep from EasyPlay?");
    r.h.get_by_label("Remove game").click();
    r.h.run();
    let id = game_id(&demo::library(now()), "Lantern Keep");
    assert!(
        r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::RemoveGame { id: g, .. } if *g == id))
    );
}

#[test]
fn context_menu_widening_preset_asks_first() {
    let mut r = rig(Route::Library);
    r.h.run();
    let card = r.h.get_by_label_contains("Lantern Keep. Offline.");
    card.click_secondary();
    r.h.run();
    r.h.get_by(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.contains("Protection")))
        .hover();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, "Online").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Give Lantern Keep internet access?");
    r.h.get_by_label("Allow").click();
    r.h.run();
    assert!(
        r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::ApplyPreset { .. }))
    );
}

#[test]
fn add_game_opens_the_picker_and_sends_open_paths() {
    let mut r = rig(Route::Library);
    r.picker
        .answer(vec!["/home/sam/Games/Celeste/Celeste.exe".into()]);
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Add game").click();
    r.h.run();
    assert_eq!(r.picker.asked(), vec![PickPurpose::AddGame]);
    assert!(r.fake.sent().contains(&UiCommand::OpenPaths {
        paths: vec!["/home/sam/Games/Celeste/Celeste.exe".into()],
        origin: OpenOrigin::Picker,
    }));
}

#[derive(Debug)]
struct TestDrop(std::path::PathBuf);

impl egui::DroppedFile for TestDrop {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        Err(String::new())
    }
}

#[test]
fn dropping_a_program_goes_to_setup_and_never_launches() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(TestDrop(
            "/home/sam/Downloads/Game.exe".into(),
        )));
    r.h.run();
    let sent = r.fake.sent();
    assert!(sent.contains(&UiCommand::OpenPaths {
        paths: vec!["/home/sam/Downloads/Game.exe".into()],
        origin: OpenOrigin::DragDrop,
    }));
    assert!(!sent.iter().any(|c| matches!(c, UiCommand::Launch { .. })));
}

#[test]
fn shortcut_sheet_opens_with_f1_and_closes_with_escape() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.key_press(Key::F1);
    r.h.run();
    assert_eq!(
        r.h.state().state.nav.top_overlay(),
        Some(Overlay::Shortcuts)
    );
    r.h.get_by_role_and_label(Role::Dialog, "Keyboard shortcuts");
    r.h.key_press(Key::Escape);
    r.h.run();
    assert!(r.h.state().state.nav.overlays().is_empty());
}

#[test]
fn empty_library_explains_and_offers_add_game() {
    let lib = easyplay_ui::bridge::library_from(Vec::new(), Vec::new(), 1, now());
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    r.h.get_by_label("Your library is empty");
    r.h.get_all_by_label("Add game").next().unwrap();
}

#[test]
fn before_the_first_snapshot_the_grid_shows_skeletons() {
    let empty = LibraryView::empty(now());
    let mut r = rig_with(
        empty,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(3);
    assert!(r.h.query_by_label_contains("Games, ").is_none());
    r.fake.publish_library(settled_library());
    r.h.run();
    r.h.get_by_label_contains("Games, 24 items");
}

#[test]
fn preview_banner_is_shown_and_dismissible() {
    let backend = demo::DemoBackend::new(now());
    let clock = std::sync::Arc::new(easyplay_testkit::FakeClock::new(now()));
    let appearance = Appearance {
        theme: ThemeChoice::Dark,
        ..Appearance::default()
    };
    let app = EasyPlayApp::new(
        UiBridge::new(Box::new(backend)),
        clock,
        appearance,
        Route::Library,
        0,
    );
    let mut h = Harness::builder()
        .with_size(Vec2::new(1280.0, 800.0))
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app);
    h.run_steps(4);
    h.get_all_by_label_contains("Preview mode").next().unwrap();
    h.get_by_label("Dismiss").click();
    h.run_steps(4);
    assert!(h.query_by_label_contains("Preview mode").is_none());
}

#[test]
fn idle_window_requests_no_repaints() {
    let mut r = rig(Route::Library);
    r.h.run();
    let steps = r.h.run();
    assert!(steps <= 1, "{steps} steps after settling");
}

#[test]
fn fake_backend_defaults_are_usable() {
    let fake = FakeBackend::default();
    assert_eq!(fake.sent(), Vec::<UiCommand>::new());
}

// ---- Snapshots (TESTING_STRATEGY §9) -----------------------------------------------------

fn snapshot_library(name: &str, size: Vec2, choice: ThemeChoice, view: ViewMode) {
    let mut r = rig_with(demo::library(now()), Route::Library, size, choice);
    r.h.state_mut().state.library.view = view;
    r.h.run_steps(8);
    r.h.snapshot(name);
}

#[test]
fn snapshot_library_dark_1280() {
    snapshot_library(
        "library_dark_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
        ViewMode::Grid,
    );
}

#[test]
fn snapshot_library_light_1280() {
    snapshot_library(
        "library_light_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Light,
        ViewMode::Grid,
    );
}

#[test]
fn snapshot_library_dark_640() {
    snapshot_library(
        "library_dark_640x480",
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
        ViewMode::Grid,
    );
}

#[test]
fn snapshot_library_high_contrast_1280() {
    snapshot_library(
        "library_hc_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::HighContrast,
        ViewMode::Grid,
    );
}

#[test]
fn snapshot_library_list_dark() {
    snapshot_library(
        "library_list_dark_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
        ViewMode::List,
    );
}

#[test]
fn snapshot_library_list_light() {
    snapshot_library(
        "library_list_light_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Light,
        ViewMode::List,
    );
}

#[test]
fn snapshot_library_selection_and_chips_dark() {
    let mut r = rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    {
        let lib = settled_library();
        let s = &mut r.h.state_mut().state.library;
        s.toggle_filter(easyplay_ui::vm::library::FilterChip::Preset(
            easyplay_core::library::PresetFilter::Offline,
        ));
        s.group = easyplay_core::library::Grouping::FirstLetter;
        s.selection = vec![game_id(&lib, "Lantern Keep"), game_id(&lib, "Moss & Iron")];
    }
    r.h.run_steps(8);
    r.h.snapshot("library_selection_dark_1280x800");
}

#[test]
fn snapshot_context_menu_dark() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.")
        .click_secondary();
    r.h.run_steps(4);
    r.h.snapshot("library_menu_dark_1280x800");
}
