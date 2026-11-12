//! Shell and library interaction tests (UI-001, UI-008) over a fake backend, plus the snapshot
//! images of TESTING_STRATEGY §9 for the shell. Every test also runs kittest's accessible-name
//! check on all input widgets.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{Activity, GameSummary};
use easyplay_testkit::FakeClock;
use easyplay_ui::bridge::{FakeBackend, LibraryView, UiBridge, UiCommand};
use easyplay_ui::route::Route;
use easyplay_ui::textures::synthetic;
use easyplay_ui::theme::{self, Appearance, ThemeChoice};
use easyplay_ui::{EasyPlayApp, demo};
use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn now() -> Timestamp {
    demo::baseline_now()
}

/// The demo library without in-progress launches, so the UI can settle (no spinners).
fn settled_library() -> LibraryView {
    let mut lib = demo::library(now());
    let games: Vec<GameSummary> = lib
        .games
        .iter()
        .cloned()
        .map(|mut g| {
            if matches!(
                g.activity,
                Activity::Launching(_) | Activity::Running { .. }
            ) {
                g.activity = Activity::Idle;
            }
            g.cover = None;
            g
        })
        .collect();
    lib.games = games.into();
    lib
}

fn harness_with(
    lib: LibraryView,
    start: Route,
    size: Vec2,
    choice: ThemeChoice,
) -> (Harness<'static, EasyPlayApp>, FakeBackend) {
    let fake = FakeBackend::new(lib);
    let clock = Arc::new(FakeClock::new(now()));
    let appearance = Appearance {
        theme: choice,
        ..Appearance::default()
    };
    let app = EasyPlayApp::new(
        UiBridge::new(Box::new(fake.clone())),
        clock,
        appearance,
        start,
        0,
    )
    .with_art(Box::new(|_| Box::new(synthetic::Inline::new())));
    let harness = Harness::builder()
        .with_size(size)
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app);
    (harness, fake)
}

fn harness(lib: LibraryView) -> (Harness<'static, EasyPlayApp>, FakeBackend) {
    harness_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    )
}

#[test]
fn boots_to_the_library_with_collection_counts() {
    let (mut h, _) = harness(settled_library());
    h.run();
    let all = demo::library(now()).games.len();
    h.get_by_label(&format!("All games, {all}"));
    h.get_by_role(Role::Navigation);
    h.get_by_label_contains("Games, 24 items");
    h.get_by_label("Search games");
    h.get_by_role_and_label(Role::Button, "Add game");
    assert_eq!(h.state().state.nav.current(), Route::Library);
}

#[test]
fn sidebar_click_selects_a_collection() {
    let (mut h, _) = harness(settled_library());
    h.run();
    h.get_by_label_contains("Favourites, ").click();
    h.run();
    assert_eq!(
        h.state().state.library.collection,
        easyplay_ui::vm::library::Collection::Favorites
    );
    h.get_by_label_contains("Games, 3 items");
}

#[test]
fn sidebar_arrow_keys_move_focus_and_enter_selects() {
    let (mut h, _) = harness(settled_library());
    h.run();
    h.get_by_label_contains("All games, ").focus();
    h.run();
    h.key_press(Key::ArrowDown);
    h.run();
    assert!(h.get_by_label_contains("Favourites, ").is_focused());
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(
        h.state().state.library.collection,
        easyplay_ui::vm::library::Collection::Favorites
    );
}

#[test]
fn ctrl_b_collapses_the_sidebar_to_a_rail() {
    let (mut h, _) = harness(settled_library());
    h.run();
    h.get_by_label("Collapse sidebar");
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    h.run();
    h.get_by_label("Expand sidebar");
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    h.run();
    h.get_by_label("Collapse sidebar");
}

#[test]
fn small_window_collapses_the_sidebar_automatically() {
    let (mut h, _) = harness_with(
        settled_library(),
        Route::Library,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    h.run();
    h.get_by_label("Expand sidebar");
    h.get_by_label_contains("Games, 24 items");
}

#[test]
fn clicking_a_card_opens_the_game_page_and_back_returns() {
    let (mut h, _) = harness(settled_library());
    h.run();
    h.get_by_label_contains("Lantern Keep. Offline.").click();
    h.run();
    assert!(matches!(h.state().state.nav.current(), Route::Game(_)));
    h.get_by_role_and_label(Role::Button, "Play Lantern Keep");
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    h.run();
    assert_eq!(h.state().state.nav.current(), Route::Library);
}

#[test]
fn keyboard_plays_and_favourites_the_focused_card() {
    let (mut h, fake) = harness(settled_library());
    h.run();
    h.get_by_label_contains("Lantern Keep. Offline.").focus();
    h.run();
    h.key_press(Key::Space);
    h.run();
    h.key_press(Key::F);
    h.run();
    let id = demo::library(now())
        .games
        .iter()
        .find(|g| &*g.name == "Lantern Keep")
        .unwrap()
        .id;
    assert_eq!(
        fake.sent(),
        vec![UiCommand::Launch(id), UiCommand::SetFavorite(id, false)]
    );
    h.get_by_role_and_label(Role::Status, "Removed Lantern Keep from favourites");
}

#[test]
fn arrow_keys_move_through_the_grid() {
    let (mut h, _) = harness(settled_library());
    h.run();
    let lib = demo::library(now());
    let first = &lib.games[0];
    h.get_by_label_contains(&format!("{}. ", first.name))
        .focus();
    h.run();
    h.key_press(Key::ArrowRight);
    h.run();
    let second = &lib.games[1];
    assert_eq!(h.state().state.library.selected, Some(second.id));
    h.key_press(Key::ArrowLeft);
    h.run();
    assert_eq!(h.state().state.library.selected, Some(first.id));
}

#[test]
fn search_with_no_results_offers_to_clear() {
    let (mut h, _) = harness(settled_library());
    h.run();
    h.get_by_label("Search games").focus();
    h.get_by_label("Search games").type_text("zzz");
    h.run();
    h.get_by_label("No games match “zzz”.");
    // Two controls clear the search: the field's ✕ and the empty state's button.
    h.get_all_by_role_and_label(Role::Button, "Clear search")
        .last()
        .unwrap()
        .click();
    h.run();
    assert!(h.state().state.library.query.is_empty());
}

#[test]
fn empty_library_explains_and_offers_add_game() {
    let lib = LibraryView {
        revision: 1,
        ..LibraryView::default()
    };
    let (mut h, _) = harness(lib);
    h.run();
    h.get_by_label("Your library is empty");
    h.get_all_by_label("Add game").next().unwrap();
}

#[test]
fn before_the_first_snapshot_the_grid_shows_skeletons() {
    let (mut h, fake) = harness(LibraryView::default());
    h.run_steps(3);
    assert!(h.query_by_label_contains("Games, ").is_none());
    fake.publish_library(settled_library());
    h.run();
    h.get_by_label_contains("Games, 24 items");
}

#[test]
fn preview_banner_is_shown_and_dismissible() {
    let backend = demo::DemoBackend::new(now());
    let clock = Arc::new(FakeClock::new(now()));
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
    let (mut h, _) = harness(settled_library());
    h.run();
    // Settled: further steps need no repaint.
    let steps = h.run();
    assert!(steps <= 1, "{steps} steps after settling");
}

// ---- Snapshots (TESTING_STRATEGY §9) -----------------------------------------------------

fn snapshot_library(name: &str, size: Vec2, choice: ThemeChoice, ppp: f32) {
    let mut lib = demo::library(now());
    // Covers come from the synthetic source; keep in-progress states (they are part of the
    // design) but render a single deterministic frame.
    let games: Vec<GameSummary> = lib.games.to_vec();
    lib.games = games.into();
    let (mut h, _) = harness_with(lib, Route::Library, size, choice);
    h.set_pixels_per_point(ppp);
    h.run_steps(8);
    h.snapshot(name);
}

#[test]
fn snapshot_library_dark_1280() {
    snapshot_library(
        "library_dark_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
        1.0,
    );
}

#[test]
fn snapshot_library_light_1280() {
    snapshot_library(
        "library_light_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Light,
        1.0,
    );
}

#[test]
fn snapshot_library_dark_640() {
    snapshot_library(
        "library_dark_640x480",
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
        1.0,
    );
}

#[test]
fn snapshot_library_high_contrast_1280() {
    snapshot_library(
        "library_hc_1280x800",
        Vec2::new(1280.0, 800.0),
        ThemeChoice::HighContrast,
        1.0,
    );
}

#[test]
fn snapshot_game_page_dark() {
    let lib = settled_library();
    let id = lib
        .games
        .iter()
        .find(|g| &*g.name == "Lantern Keep")
        .unwrap()
        .id;
    let (mut h, _) = harness_with(
        lib,
        Route::Game(id),
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    h.run_steps(8);
    h.snapshot("game_page_dark_1280x800");
}
