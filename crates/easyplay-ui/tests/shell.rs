//! Shell and library interaction tests (UI-001, UI-008 … UI-013, UI-030, UI-031) over a fake
//! backend: the top bar, the filter line (scopes, filters, the sort, the selection), the
//! wall and the list, the small library, the reflow between them; and the snapshot images
//! of TESTING_STRATEGY §9 for the library.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use common::{Rig, game_id, now, rebuild, rig, rig_with, settled_library};
use easyplay_core::error::{ErrorCode, catalog};
use easyplay_core::ids::GameId;
use easyplay_core::library::SortKey;
use easyplay_core::model::{Activity, Availability, EntryKind};
use easyplay_core::service::{Diagnostics, RecoveryReason, RecoveryState};
use easyplay_testkit::FakeClock;
use easyplay_ui::bridge::{
    FakeBackend, LibraryView, OpenOrigin, UiBridge, UiCommand, library_from,
};
use easyplay_ui::input::picker::FakePicker;
use easyplay_ui::route::{Overlay, Route};
use easyplay_ui::screens::library::grid::last_drawn;
use easyplay_ui::textures::synthetic;
use easyplay_ui::theme::tokens::{control, layout};
use easyplay_ui::theme::{self, Appearance, ThemeChoice};
use easyplay_ui::vm::library::{Collection, CollectionExt, Group, ViewMode};
use easyplay_ui::{EasyPlayApp, demo};
use egui::accesskit::Role;
use egui::{Color32, Key, Modifiers, Rect, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

const WINDOW: Vec2 = Vec2::new(1280.0, 800.0);

/// The colour of a rendered image's pixel at a point (the rigs' scale is 1).
macro_rules! px {
    ($image:expr, $at:expr) => {{
        let at: egui::Pos2 = $at;
        let p = $image.get_pixel(at.x as u32, at.y as u32).0;
        Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])
    }};
}

/// Whether two colours are the same within the rasteriser's rounding.
fn near(a: Color32, b: Color32) -> bool {
    a.to_array()
        .iter()
        .zip(b.to_array())
        .all(|(x, y)| x.abs_diff(y) <= 24)
}

/// The first `n` games of the settled sample library.
fn first(n: usize) -> LibraryView {
    let full = settled_library();
    library_from(
        full.games.iter().take(n).cloned().collect(),
        demo::sample_tags(),
        1,
        now(),
    )
}

/// A rig over `lib` stepping at 60 frames a second, to watch motion frame by frame.
fn rig_60(lib: LibraryView, appearance: Appearance) -> Rig {
    let fake = FakeBackend::new(lib);
    let picker = FakePicker::default();
    let clock = Arc::new(FakeClock::new(now()));
    let app = EasyPlayApp::new(
        UiBridge::new(Box::new(fake.clone())),
        clock.clone(),
        appearance,
        Route::Library,
        0,
    )
    .with_art(Box::new(|_| Box::new(synthetic::Inline::new())))
    .with_picker(Box::new(picker.clone()));
    let h = Harness::builder()
        .with_size(WINDOW)
        .with_step_dt(1.0 / 60.0)
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app);
    Rig {
        h,
        fake,
        picker,
        clock,
    }
}

/// The games in the order the page shows them.
fn shown(r: &mut Rig) -> Vec<GameId> {
    let lib = r.h.state().bridge().library().clone();
    let shelf = r.h.state_mut().state.library.result(&lib);
    shelf
        .items
        .iter()
        .map(|i| lib.games[*i as usize].id)
        .collect()
}

/// The start of the name a game's cover answers to.
fn cover_label(r: &Rig, id: GameId) -> String {
    let lib = r.h.state().bridge().library().clone();
    format!("{}. ", lib.get(id).unwrap().name)
}

// ---- The top bar ---------------------------------------------------------------------------

#[test]
fn boots_to_the_library_with_its_scopes_and_counts() {
    let mut r = rig(Route::Library);
    r.h.run();
    let all = demo::library(now()).games.len();
    r.h.get_by_role_and_label(Role::Switch, &format!("All, {all}"));
    r.h.get_by_role_and_label(Role::Navigation, "Main");
    r.h.get_by_label_contains("Games, 24 items");
    r.h.get_by_label("Search games");
    r.h.get_by_role_and_label(Role::Button, "Add game");
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn the_top_bar_goes_to_the_three_destinations() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Statistics")
        .click();
    common::settle(&mut r);
    assert_eq!(r.h.state().state.nav.current(), Route::Statistics);
    r.h.get_by_role_and_label(Role::Button, "Settings").click();
    common::settle(&mut r);
    assert!(matches!(
        r.h.state().state.nav.current(),
        Route::Settings(_)
    ));
    r.h.get_by_role_and_label(Role::Button, "Library").click();
    common::settle(&mut r);
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn the_running_game_has_a_pill_in_the_top_bar_that_opens_and_stops_it() {
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Lantern Keep" {
            g.activity = Activity::Running {
                since: now().saturating_sub(std::time::Duration::from_secs(42)),
            };
        }
    });
    let id = game_id(&lib, "Lantern Keep");
    let mut r = rig_with(lib, Route::Library, WINDOW, ThemeChoice::Dark);
    r.h.run_steps(4);
    // The pill's round button, in the top bar, stops the game (the cover's disc has the
    // same name).
    let stop =
        r.h.get_all_by_role_and_label(Role::Button, "Stop Lantern Keep")
            .find(|n| n.rect().bottom() <= layout::TOP_BAR)
            .expect("the pill's stop button");
    stop.click();
    r.h.run_steps(2);
    assert!(
        r.fake
            .sent()
            .contains(&UiCommand::Stop { id, force: false })
    );
    // Its title opens the game page.
    r.h.get_by_role_and_label(Role::Button, "Open the page of Lantern Keep")
        .click();
    r.h.run_steps(4);
    assert_eq!(r.h.state().state.nav.current(), Route::Game(id));
}

#[test]
fn a_damaged_library_offers_no_search_and_no_covers_or_list() {
    let r = &mut rig(Route::Library);
    let error = catalog::describe(
        &easyplay_core::error::Error::new(ErrorCode::DbCorrupt),
        &catalog::DescribeCtx::plain(),
    );
    r.fake.publish_diagnostics(Diagnostics {
        recovery: Some(RecoveryState {
            reason: RecoveryReason::Damaged {
                backups: Vec::new(),
                repaired_games: Some(9),
                missing_games: Vec::new(),
            },
            error,
        }),
        ..Diagnostics::default()
    });
    common::settle(r);
    r.h.get_all_by_label_contains("The library file is damaged")
        .next()
        .unwrap();
    assert!(r.h.query_by_label("Search games").is_none());
    assert!(
        r.h.query_by_role_and_label(Role::RadioGroup, "Show as")
            .is_none()
    );
    assert!(
        r.h.query_by_role_and_label(Role::Button, "Add game")
            .is_none()
    );
}

#[test]
fn a_small_window_keeps_the_top_bar_and_the_wall() {
    let mut r = rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    // "Add game" turns into its icon (its label is the tooltip and the name).
    let add = r.h.get_by_role_and_label(Role::Button, "Add game").rect();
    assert!((add.width() - control::BUTTON).abs() < 0.5, "{add:?}");
    r.h.get_by_label("Search games");
    r.h.get_by_label_contains("Games, 24 items");
}

// ---- The filter line -----------------------------------------------------------------------

#[test]
fn a_scope_word_shows_its_games() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Favourites, ").click();
    r.h.run();
    assert_eq!(r.h.state().state.library.collection, Collection::Favorites);
    r.h.get_by_label_contains("Games, 3 items");
}

#[test]
fn a_scope_word_answers_the_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label("Favourites, 3").focus();
    r.h.run();
    r.h.key_press(Key::Enter);
    r.h.run();
    assert_eq!(r.h.state().state.library.collection, Collection::Favorites);
    r.h.get_by_label("Recent, 12").focus();
    r.h.run();
    r.h.key_press(Key::Space);
    r.h.run();
    assert_eq!(r.h.state().state.library.collection, Collection::Recent);
}

#[test]
fn the_filter_line_offers_the_scopes_that_hold_something() {
    // Every game is present and a game: no "Missing", so no "Installed", no "Installers".
    let lib = rebuild(&settled_library(), |g| {
        g.availability = Availability::Present;
        g.kind = EntryKind::Game;
    });
    let mut r = rig_with(lib.clone(), Route::Library, WINDOW, ThemeChoice::Dark);
    r.h.run();
    let offered = r.h.state_mut().state.library.shown_scopes(&lib);
    for c in [
        Collection::All,
        Collection::Favorites,
        Collection::Recent,
        Collection::NeverPlayed,
    ] {
        assert!(offered.contains(&c), "{c:?} is always offered");
    }
    for c in [
        Collection::Missing,
        Collection::Installed,
        Collection::Installers,
    ] {
        assert!(!offered.contains(&c), "{c:?}");
        let word = format!("{}, ", c.label());
        assert!(r.h.query_by_label_contains(&word).is_none(), "{word}");
    }
    // In the sample library a game is missing and one is an installer: their words appear.
    let mut r = rig(Route::Library);
    r.h.run();
    for word in ["Installed, ", "Missing, ", "Installers, "] {
        r.h.get_by_label_contains(word);
    }
}

#[test]
fn in_a_narrow_window_the_scopes_fold_into_one_word_with_a_menu() {
    let mut r = rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    // Only the current scope shows, so that "Filter" and the sort stay in reach.
    assert!(r.h.query_by_label_contains("Favourites, ").is_none());
    r.h.get_by_role_and_label(Role::Button, "Filter");
    r.h.get_by_role_and_label(Role::Button, "Recently played");
    r.h.get_by_label("All, 24").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, "Favourites")
        .click();
    r.h.run_steps(8);
    assert_eq!(r.h.state().state.library.collection, Collection::Favorites);
    r.h.get_by_label("Favourites, 3");
}

#[test]
fn search_with_no_results_offers_to_clear() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label("Search games").focus();
    r.h.get_by_label("Search games").type_text("zzz");
    r.h.run();
    r.h.get_by_label("Nothing matches “zzz”");
    r.h.get_by_role_and_label(Role::Button, "Clear").click();
    r.h.run_steps(8);
    assert!(r.h.state().state.library.query.is_empty());
    // The field's own clear button does the same.
    r.h.get_by_label("Search games").focus();
    r.h.get_by_label("Search games").type_text("zzz");
    r.h.run();
    r.h.get_all_by_role_and_label(Role::Button, "Clear search")
        .last()
        .unwrap()
        .click();
    r.h.run_steps(8);
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
fn filters_turn_on_in_their_menu_and_off_by_their_word() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Filter").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItemCheckBox, "Online")
        .click();
    r.h.run();
    r.h.key_press(Key::Escape);
    r.h.run_steps(8);
    assert_eq!(r.h.state().state.library.chips().len(), 1);
    r.h.get_by_label("Remove Online").click();
    r.h.run();
    assert!(r.h.state().state.library.chips().is_empty());
}

#[test]
fn the_sort_word_sorts_and_groups() {
    let mut r = rig(Route::Library);
    r.h.run();
    assert_eq!(r.h.state().state.library.sort, SortKey::LastPlayed);
    r.h.get_by_role_and_label(Role::Button, "Recently played")
        .click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItemRadio, "Name")
        .click();
    r.h.run_steps(8);
    assert_eq!(r.h.state().state.library.sort, SortKey::Name);
    r.h.get_by_role_and_label(Role::Button, "Name").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItemRadio, "First letter")
        .click();
    r.h.run_steps(8);
    assert_eq!(r.h.state().state.library.group, Group::Letter);
}

#[test]
fn escape_clears_the_search_then_the_selection() {
    let mut r = rig(Route::Library);
    r.h.run();
    let ids = shown(&mut r);
    r.h.state_mut().state.library.selection = vec![ids[0], ids[1]];
    r.h.state_mut().state.library.query = "a".into();
    r.h.run();
    r.h.key_press(Key::Escape);
    r.h.run();
    assert!(r.h.state().state.library.query.is_empty());
    assert_eq!(r.h.state().state.library.selection.len(), 2);
    r.h.key_press(Key::Escape);
    r.h.run();
    assert!(r.h.state().state.library.selection.is_empty());
}

// ---- The wall ------------------------------------------------------------------------------

#[test]
fn clicking_a_cover_opens_the_game_page_and_back_returns() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.").click();
    r.h.run_steps(30);
    assert!(matches!(r.h.state().state.nav.current(), Route::Game(_)));
    r.h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    r.h.run_steps(30);
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn space_selects_the_focused_cover_and_ctrl_enter_plays_it() {
    let mut r = rig(Route::Library);
    r.h.run();
    let id = game_id(&settled_library(), "Lantern Keep");
    r.h.get_by_label_contains("Lantern Keep. Offline.").focus();
    r.h.run();
    r.h.key_press(Key::Space);
    r.h.run();
    assert_eq!(r.h.state().state.library.selection, vec![id]);
    assert!(r.fake.sent().is_empty(), "Space does not play");
    r.h.key_press(Key::Space);
    r.h.run();
    assert!(r.h.state().state.library.selection.is_empty());
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    r.h.run();
    assert_eq!(r.fake.sent(), vec![UiCommand::Launch { id, exe: None }]);
}

#[test]
fn f_favourites_the_focused_cover_and_says_so() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.").focus();
    r.h.run();
    r.h.key_press(Key::F);
    r.h.run();
    let id = game_id(&demo::library(now()), "Lantern Keep");
    assert_eq!(
        r.fake.sent(),
        vec![UiCommand::SetFavorite {
            id,
            favorite: false
        }]
    );
    // The toast, and the live-region announcement of it (ACCESSIBILITY §4.3).
    assert_eq!(
        r.h.query_all_by_role_and_label(Role::Status, "Removed from Favourites")
            .count(),
        2
    );
    assert_eq!(
        easyplay_ui::a11y::Announcer::current(&r.h.ctx).as_deref(),
        Some("Removed from Favourites")
    );
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
fn arrow_keys_move_through_the_wall_in_its_order() {
    let mut r = rig(Route::Library);
    r.h.run();
    let order = shown(&mut r);
    let label = cover_label(&r, order[0]);
    r.h.get_by_label_contains(&label).focus();
    r.h.run();
    r.h.key_press(Key::ArrowRight);
    r.h.run();
    assert_eq!(r.h.state().state.library.selected, Some(order[1]));
    // Six covers to a row at 1280.
    r.h.key_press(Key::ArrowDown);
    r.h.run();
    assert_eq!(r.h.state().state.library.selected, Some(order[7]));
    r.h.key_press(Key::ArrowLeft);
    r.h.run();
    assert_eq!(r.h.state().state.library.selected, Some(order[6]));
}

#[test]
fn ctrl_click_and_shift_click_build_a_selection_whose_line_acts_on_it() {
    let mut r = rig(Route::Library);
    r.h.run();
    let order = shown(&mut r);
    let a = cover_label(&r, order[0]);
    let b = cover_label(&r, order[3]);
    r.h.get_by_label_contains(&a)
        .click_modifiers(Modifiers::COMMAND);
    r.h.run();
    r.h.get_by_label_contains(&b)
        .click_modifiers(Modifiers::SHIFT);
    r.h.run();
    assert_eq!(r.h.state().state.library.selection.len(), 4);
    // The line carries the selection's count and actions instead of the scopes.
    r.h.run_steps(8);
    r.h.get_by_label("4 selected");
    assert!(r.h.query_by_label_contains("All, ").is_none());
    r.h.get_by_role_and_label(Role::Button, "Favourite").click();
    r.h.run();
    let favs = r
        .fake
        .sent()
        .iter()
        .filter(|c| matches!(c, UiCommand::SetFavorite { favorite: true, .. }))
        .count();
    assert_eq!(favs, 4, "{:?}", r.fake.sent());
    r.h.get_by_role_and_label(Role::Button, "Hide").click();
    r.h.run();
    let hidden = r
        .fake
        .sent()
        .iter()
        .filter(|c| matches!(c, UiCommand::SetHidden { hidden: true, .. }))
        .count();
    assert_eq!(hidden, 4);
}

#[test]
fn the_selection_line_clears_with_its_cross() {
    let mut r = rig(Route::Library);
    r.h.run();
    let order = shown(&mut r);
    r.h.state_mut().state.library.selection = vec![order[0], order[1]];
    r.h.run_steps(8);
    r.h.get_by_label("2 selected");
    r.h.get_by_role_and_label(Role::Button, "Clear selection")
        .click();
    r.h.run_steps(8);
    assert!(r.h.state().state.library.selection.is_empty());
    r.h.get_by_label_contains("All, ");
}

#[test]
fn a_plain_click_toggles_a_cover_while_something_is_selected() {
    let mut r = rig(Route::Library);
    r.h.run();
    let order = shown(&mut r);
    let a = cover_label(&r, order[0]);
    let b = cover_label(&r, order[2]);
    r.h.get_by_label_contains(&a)
        .click_modifiers(Modifiers::COMMAND);
    r.h.run();
    r.h.get_by_label_contains(&b).click();
    r.h.run();
    assert_eq!(
        r.h.state().state.library.selection,
        vec![order[0], order[2]]
    );
    r.h.get_by_label_contains(&a).click();
    r.h.run();
    assert_eq!(r.h.state().state.library.selection, vec![order[2]]);
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn removing_the_selection_asks_once_with_cancel_as_default() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    r.h.run_steps(8);
    r.h.get_by_role_and_label(Role::Button, "Remove…").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Remove 24 games?");
    // Enter activates the safe default.
    r.h.key_press(Key::Enter);
    r.h.run_steps(8);
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::RemoveGame { .. }))
    );
    assert!(r.h.state().state.nav.overlays().is_empty());
}

#[test]
fn the_game_menu_opens_by_keyboard_and_removes_with_confirmation() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.").focus();
    r.h.run();
    r.h.key_press_modifiers(Modifiers::SHIFT, Key::F10);
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, "Open game page");
    r.h.get_by_role_and_label(Role::MenuItem, "Remove…").click();
    r.h.run_steps(8);
    r.h.get_by_role_and_label(Role::Dialog, "Remove Lantern Keep?");
    r.h.get_by_label("Remove game").click();
    r.h.run_steps(8);
    let id = game_id(&demo::library(now()), "Lantern Keep");
    assert!(
        r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::RemoveGame { id: g, .. } if *g == id))
    );
}

#[test]
fn the_game_menu_widening_protection_asks_first() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.")
        .click_secondary();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, "Protection")
        .click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItemCheckBox, "Online")
        .click();
    r.h.run_steps(8);
    r.h.get_by_role_and_label(Role::Dialog, "Give Lantern Keep internet access?");
    r.h.get_by_label("Allow").click();
    r.h.run_steps(8);
    assert!(
        r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::ApplyPreset { .. }))
    );
}

#[test]
fn the_game_menu_ticks_tags_and_makes_new_ones_in_place() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. Offline.")
        .click_secondary();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, "Tags").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItemCheckBox, "co-op")
        .click();
    r.h.run();
    // The menu stays open, with a field for a new tag.
    let field = easyplay_ui::strings::tags::NEW_TAG_NAME;
    r.h.get_by_label(field()).focus();
    r.h.get_by_label(field()).type_text("speedrun");
    r.h.key_press(Key::Enter);
    r.h.run();
    let id = game_id(&settled_library(), "Lantern Keep");
    let sets: Vec<Vec<String>> = r
        .fake
        .sent()
        .iter()
        .filter_map(|c| match c {
            UiCommand::SetTags { id: g, tags } if *g == id => {
                Some(tags.iter().map(|t| t.to_string()).collect())
            }
            _ => None,
        })
        .collect();
    assert!(
        sets.iter().any(|t| t.iter().any(|n| n == "co-op")),
        "{sets:?}"
    );
    assert!(
        sets.iter().any(|t| t.iter().any(|n| n == "speedrun")),
        "{sets:?}"
    );
}

#[test]
fn add_game_opens_the_add_a_game_dialog_at_its_first_step() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Add game").click();
    r.h.run_steps(4);
    assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
    assert!(r.h.state().state.add_game.is_some());
    assert!(r.picker.asked().is_empty(), "no file picker first");
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
        .push(Arc::new(TestDrop("/home/sam/Downloads/Game.exe".into())));
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
    r.h.run_steps(8);
    assert!(r.h.state().state.nav.overlays().is_empty());
}

// ---- The list ------------------------------------------------------------------------------

#[test]
fn list_view_sorts_by_header_and_flips() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    r.h.run();
    assert_eq!(r.h.state().state.library.view, ViewMode::List);
    r.h.get_by_role_and_label(Role::ColumnHeader, "Time played")
        .click();
    r.h.run();
    assert_eq!(r.h.state().state.library.sort, SortKey::Playtime);
    assert!(r.h.state().state.library.descending);
    r.h.get_by_role_and_label(Role::ColumnHeader, "Sorted by Time played, descending")
        .click();
    r.h.run();
    assert!(!r.h.state().state.library.descending);
}

// ---- Small libraries -----------------------------------------------------------------------

#[test]
fn an_empty_library_invites_the_first_game() {
    let lib = library_from(Vec::new(), Vec::new(), 1, now());
    let mut r = rig_with(lib, Route::Library, WINDOW, ThemeChoice::Dark);
    r.h.run();
    r.h.get_by_label("Add your first game");
    // No search over nothing; the one "Add game" is the Play-sized pill.
    assert!(r.h.query_by_label("Search games").is_none());
    let adds: Vec<_> =
        r.h.get_all_by_role_and_label(Role::Button, "Add game")
            .collect();
    assert_eq!(adds.len(), 1);
    assert_eq!(adds[0].rect().height(), control::PLAY);
    adds[0].click();
    r.h.run_steps(4);
    assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
}

/// A small library of `n` games at rest, and its covers' rects on the screen.
fn small(n: usize) -> (Rig, Vec<Rect>) {
    let mut r = rig_with(first(n), Route::Library, WINDOW, ThemeChoice::Dark);
    r.h.run();
    let order = shown(&mut r);
    let rects = order
        .iter()
        .map(|id| {
            let label = cover_label(&r, *id);
            r.h.get_by_label_contains(&label).rect()
        })
        .collect();
    (r, rects)
}

#[test]
fn small_libraries_show_larger_covers_with_captions_and_an_add_game_tile() {
    for (n, w, play) in [
        (1, layout::COVER_ONE, true),
        (3, layout::COVER_FEW, true),
        (6, layout::COVER_SPARSE, false),
    ] {
        let (mut r, rects) = small(n);
        let p = theme::theme(&r.h.ctx).palette.clone();
        let image = r.h.render().expect("render");
        assert_eq!(rects.len(), n);
        // The covers whose captions are in the window.
        let seen = rects
            .iter()
            .filter(|c| c.bottom() + layout::CAPTION_GAP + layout::CAPTION_LINE < WINDOW.y);
        for c in seen {
            assert!((c.width() - w).abs() < 0.5, "{n} games: {c:?}");
            // The caption always shows: text under the cover's left part.
            let y = c.bottom() + layout::CAPTION_GAP + layout::CAPTION_LINE / 2.0;
            assert!(
                (0..40).any(|dx| !near(px!(image, pos2(c.left() + dx as f32, y)), p.bg)),
                "{n} games: no caption under {c:?}"
            );
            // Play always shows on the covers of up to three games.
            let disc = pos2(
                c.right() - control::DISC_BIG_INSET - control::DISC_BIG / 2.0,
                c.top() + control::DISC_BIG_INSET + control::DISC_BIG / 4.0,
            );
            assert_eq!(near(px!(image, disc), p.fill), play, "{n} games: Play");
        }
        // The dashed "Add game" tile ends the wall, and the top bar leaves it out.
        let adds: Vec<_> =
            r.h.get_all_by_role_and_label(Role::Button, "Add game")
                .collect();
        assert_eq!(adds.len(), 1, "{n} games");
        let tile = adds[0].rect();
        assert!((tile.width() - w).abs() < 0.5, "{n} games: {tile:?}");
        let last = rects[n - 1];
        assert!(tile.left() > last.left() || tile.top() > last.top());
    }
}

#[test]
fn up_to_three_games_stand_in_the_middle_of_the_window() {
    let (r, rects) = small(3);
    let tile = r.h.get_by_role_and_label(Role::Button, "Add game").rect();
    // Three covers and the tile, centred together.
    let mid = (rects[0].left() + tile.right()) / 2.0;
    assert!((mid - WINDOW.x / 2.0).abs() < 1.0, "{mid}");
}

#[test]
fn the_dashed_tile_opens_the_add_a_game_dialog() {
    let (mut r, _) = small(5);
    r.h.get_by_role_and_label(Role::Button, "Add game").click();
    r.h.run_steps(4);
    assert_eq!(r.h.state().state.nav.top_overlay(), Some(Overlay::AddGame));
}

// ---- Captions and rings at the page's edges --------------------------------------------------

#[test]
fn a_caption_near_the_right_edge_hangs_from_the_covers_right_side() {
    // The last cover of the first row has a long title.
    let base = settled_library();
    let mut probe = rig_with(base.clone(), Route::Library, WINDOW, ThemeChoice::Dark);
    probe.h.run();
    let last_in_row = shown(&mut probe)[5];
    let long = "The Longest Winter Road: The Director's Edition";
    let lib = rebuild(&base, |g| {
        if g.id == last_in_row {
            g.name = long.into();
        }
    });
    let mut r = rig_with(lib, Route::Library, WINDOW, ThemeChoice::Dark);
    r.h.run();
    let cover = r.h.get_by_label_contains(&format!("{long}. ")).rect();
    r.h.event(egui::Event::PointerMoved(cover.center()));
    r.h.run();
    let image = r.h.render().expect("render");
    let bg = theme::theme(&r.h.ctx).palette.bg;
    // The caption hangs 9 under the cover (which does not rise with it).
    let y = cover.bottom() + layout::CAPTION_GAP + layout::CAPTION_LINE / 2.0;
    let inked = |x0: f32, x1: f32| {
        let mut x = x0;
        while x < x1 {
            if !near(px!(image, pos2(x, y)), bg) {
                return true;
            }
            x += 1.0;
        }
        false
    };
    assert!(
        // Up to the scroll bar, which stays visible at the window's right edge.
        !inked(cover.right() + 2.0, WINDOW.x - 12.0),
        "the caption runs past the cover's right side"
    );
    assert!(
        inked(cover.left() - 60.0, cover.left()),
        "the long caption does not reach left of the cover"
    );
}

#[test]
fn the_first_rows_ring_is_whole_and_nothing_shows_above_the_page_once_scrolled() {
    let mut r = rig(Route::Library);
    r.h.run();
    let order = shown(&mut r);
    let label = cover_label(&r, order[0]);
    r.h.get_by_label_contains(&label).focus();
    r.h.run();
    r.h.key_press(Key::ArrowRight);
    r.h.run();
    r.h.key_press(Key::ArrowLeft);
    r.h.run();
    let cover =
        r.h.state()
            .state
            .library
            .cursor_rect
            .expect("focused cover");
    let accent = theme::theme(&r.h.ctx).palette.accent_mark;
    let bg = theme::theme(&r.h.ctx).palette.bg;
    let image = r.h.render().expect("render");
    // A cover with keyboard focus rises 3; its ring stands 3 out, 2 wide: 4 above the
    // drawn cover is the middle of the ring, above the page's top edge.
    let ring = px!(
        image,
        pos2(cover.center().x, cover.top() - layout::COVER_LIFT - 4.0)
    );
    assert!(near(ring, accent), "{ring:?} is not the ring's {accent:?}");
    // Scrolled so that the first row runs under the page's top edge: nothing of it shows
    // above that edge.
    let page_top = cover.top() - layout::PAGE_PAD[0];
    r.h.event(egui::Event::PointerMoved(pos2(640.0, 500.0)));
    r.h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -cover.height() / 3.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    r.h.run();
    let image = r.h.render().expect("render");
    for y in 1..8 {
        for x in (40..1240).step_by(20) {
            let at = pos2(x as f32, page_top - y as f32);
            let c = px!(image, at);
            assert!(near(c, bg), "{c:?} at {at:?} above the page");
        }
    }
}

// ---- Reflow --------------------------------------------------------------------------------

/// Whether `m` lies between `a` and `b`, edge by edge, and strictly between them in width.
fn between(a: Rect, m: Rect, b: Rect) -> bool {
    let within = |x: f32, p: f32, q: f32| x >= p.min(q) - 0.5 && x <= p.max(q) + 0.5;
    let edges = |r: Rect| [r.left(), r.top(), r.right(), r.bottom()];
    let (ea, em, eb) = (edges(a), edges(m), edges(b));
    (0..4).all(|i| within(em[i], ea[i], eb[i]))
        && m.width() > a.width().min(b.width()) + 0.5
        && m.width() < a.width().max(b.width()) - 0.5
}

/// Switch from `from` to `to` and follow the first cover frame by frame: it travels and
/// resizes from where it was to where it goes, closer every frame, arriving at 400 ms.
/// `prepare` sets the page up first (a search, groups).
fn travels(lib: LibraryView, from: ViewMode, to: ViewMode, prepare: impl FnOnce(&mut Rig)) {
    let mut r = rig_60(lib, Appearance::default());
    r.h.state_mut().state.library.view = from;
    prepare(&mut r);
    r.h.run_steps(60);
    let id = shown(&mut r)[0];
    let start = last_drawn(&r.h.ctx, id).expect("drawn before");
    let label = if to == ViewMode::Grid {
        "Covers"
    } else {
        "List"
    };
    r.h.get_by_label(label).click();
    r.h.step();
    let mut path = Vec::new();
    for _ in 0..30 {
        r.h.step();
        path.push(last_drawn(&r.h.ctx, id).expect("drawn in flight"));
    }
    r.h.run_steps(30);
    let end = last_drawn(&r.h.ctx, id).expect("drawn after");
    assert!(start != end, "{start:?} → {end:?}");
    // A few frames in it is between its two places, its size too.
    for m in &path[1..10] {
        assert!(between(start, *m, end), "{start:?} → {m:?} → {end:?}");
    }
    // Closer every frame, and not there before the journey's end.
    let dist = |m: &Rect| (m.min - end.min).length() + (m.max - end.max).length();
    for w in path.windows(2) {
        assert!(dist(&w[1]) <= dist(&w[0]) + 0.01, "{path:?}");
    }
    assert!(dist(&path[16]) > 0.5, "arrived early: {path:?}");
}

#[test]
fn switching_covers_to_list_carries_each_cover_into_its_row() {
    travels(settled_library(), ViewMode::Grid, ViewMode::List, |_| {});
    // A library whose list fits the page and whose wall does not: the page's scroll bar
    // comes or goes as the view changes.
    travels(first(13), ViewMode::Grid, ViewMode::List, |_| {});
}

#[test]
fn switching_list_to_covers_carries_each_thumb_onto_the_wall() {
    travels(settled_library(), ViewMode::List, ViewMode::Grid, |_| {});
    travels(first(13), ViewMode::List, ViewMode::Grid, |_| {});
}

#[test]
fn the_switch_carries_covers_with_groups_and_with_a_search() {
    for (from, to) in [
        (ViewMode::Grid, ViewMode::List),
        (ViewMode::List, ViewMode::Grid),
    ] {
        travels(settled_library(), from, to, |r| {
            r.h.state_mut().state.library.group = Group::Letter;
        });
        travels(settled_library(), from, to, |r| {
            r.h.state_mut().state.library.query = "a".into();
        });
    }
}

#[test]
fn after_the_list_scrolled_only_rows_on_the_page_carry_their_covers() {
    let mut r = rig_60(settled_library(), Appearance::default());
    r.h.state_mut().state.library.view = ViewMode::List;
    r.h.run_steps(30);
    r.h.event(egui::Event::PointerMoved(pos2(640.0, 500.0)));
    r.h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -400.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    });
    r.h.run_steps(60);
    let order = shown(&mut r);
    // The first rows scrolled away: their thumbs are not laid out.
    assert!(last_drawn(&r.h.ctx, order[0]).is_none());
    let before: Vec<(GameId, Option<Rect>)> = order
        .iter()
        .map(|id| (*id, last_drawn(&r.h.ctx, *id)))
        .collect();
    r.h.get_by_label("Covers").click();
    r.h.step();
    for _ in 0..4 {
        r.h.step();
    }
    let mid: Vec<Option<Rect>> = order.iter().map(|id| last_drawn(&r.h.ctx, *id)).collect();
    r.h.run_steps(60);
    let end: Vec<Option<Rect>> = order.iter().map(|id| last_drawn(&r.h.ctx, *id)).collect();
    let mut carried = 0;
    let mut arrived = 0;
    for (i, (_, thumb)) in before.iter().enumerate() {
        let (Some(m), Some(e)) = (mid[i], end[i]) else {
            continue;
        };
        match thumb {
            // A row on the page: its thumb grows into the cover.
            Some(t) => {
                assert!(between(*t, m, e), "{t:?} → {m:?} → {e:?}");
                carried += 1;
            }
            // A row that was off the page: its cover rises in where it stands, never a
            // thumb.
            None => {
                assert!(m.width() > layout::LIST_THUMB * 4.0, "{m:?}");
                arrived += 1;
            }
        }
    }
    assert!(
        carried > 0 && arrived > 0,
        "{carried} carried, {arrived} arrived"
    );
}

#[test]
fn under_reduced_motion_the_switch_does_not_travel() {
    let appearance = Appearance {
        reduce_motion: true,
        ..Appearance::default()
    };
    let mut r = rig_60(settled_library(), appearance);
    r.h.state_mut().state.library.view = ViewMode::List;
    r.h.run_steps(30);
    let id = shown(&mut r)[0];
    r.h.get_by_label("Covers").click();
    r.h.step();
    r.h.step();
    let at = last_drawn(&r.h.ctx, id).expect("drawn");
    r.h.run_steps(30);
    let end = last_drawn(&r.h.ctx, id).expect("drawn");
    assert_eq!(at, end, "it stands in its place at once");
}

#[test]
fn a_reflow_restarts_from_where_a_cover_is() {
    let mut r = rig_60(settled_library(), Appearance::default());
    r.h.run_steps(60);
    let id = shown(&mut r)[0];
    let home = last_drawn(&r.h.ctx, id).expect("drawn");
    // Sort by name: the cover sets out for its new place.
    r.h.state_mut().state.library.set_sort(SortKey::Name, false);
    for _ in 0..6 {
        r.h.step();
    }
    let mid = last_drawn(&r.h.ctx, id).expect("in flight");
    assert!(mid != home, "it set out");
    // Back to the first order before it arrives: it turns back from where it is.
    r.h.state_mut()
        .state
        .library
        .set_sort(SortKey::LastPlayed, false);
    r.h.step();
    let next = last_drawn(&r.h.ctx, id).expect("turning");
    assert!(
        (next.min - mid.min).length() < 40.0,
        "jumped from {mid:?} to {next:?}"
    );
    r.h.run_steps(60);
    assert_eq!(last_drawn(&r.h.ctx, id), Some(home));
}

// ---- Misc ----------------------------------------------------------------------------------

#[test]
fn before_the_first_snapshot_the_wall_shows_loading_covers() {
    let empty = LibraryView::empty(now());
    let mut r = rig_with(empty, Route::Library, WINDOW, ThemeChoice::Dark);
    r.h.run_steps(3);
    assert!(r.h.query_by_label_contains("Games, ").is_none());
    r.fake.publish_library(settled_library());
    r.h.run_steps(4);
    r.h.get_by_label_contains("Games, 24 items");
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
        .with_size(WINDOW)
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
        WINDOW,
        ThemeChoice::Dark,
        ViewMode::Grid,
    );
}

#[test]
fn snapshot_library_light_1280() {
    snapshot_library(
        "library_light_1280x800",
        WINDOW,
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
        WINDOW,
        ThemeChoice::HighContrast,
        ViewMode::Grid,
    );
}

#[test]
fn snapshot_library_list_dark() {
    snapshot_library(
        "library_list_dark_1280x800",
        WINDOW,
        ThemeChoice::Dark,
        ViewMode::List,
    );
}

#[test]
fn snapshot_library_list_light() {
    snapshot_library(
        "library_list_light_1280x800",
        WINDOW,
        ThemeChoice::Light,
        ViewMode::List,
    );
}

#[test]
fn snapshot_library_selection_and_filters_dark() {
    let mut r = rig_with(settled_library(), Route::Library, WINDOW, ThemeChoice::Dark);
    {
        let lib = settled_library();
        let s = &mut r.h.state_mut().state.library;
        s.toggle_filter(easyplay_ui::vm::library::FilterChip::Preset(
            easyplay_core::library::PresetFilter::Offline,
        ));
        s.group = Group::Letter;
        s.selection = vec![
            game_id(&lib, "Clockwork Garden"),
            game_id(&lib, "Driftwood"),
        ];
    }
    r.h.run_steps(8);
    r.h.snapshot("library_selection_dark_1280x800");
}

#[test]
fn snapshot_small_library_dark() {
    let mut r = rig_with(first(3), Route::Library, WINDOW, ThemeChoice::Dark);
    r.h.run_steps(8);
    r.h.snapshot("library_small_dark_1280x800");
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
