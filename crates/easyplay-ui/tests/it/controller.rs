//! UX-001 / UX-002: controller navigation with synthetic controller input (UF-15), and grid
//! focus rules (roving focus, following by ID, return after a game page).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use common::{Rig, rig, settle};
use easyplay_core::ids::GameId;
use easyplay_core::launch::LaunchState;
use easyplay_ui::bridge::{Command, UiCommand, UiEvent};
use easyplay_ui::input::gamepad::{PadButton, PadInput};
use easyplay_ui::route::Route;
use egui::Key;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};

/// Feed controller events through the app's `raw_input_hook` path, then run.
fn pump(r: &mut Rig, events: &[PadInput]) {
    for e in events {
        r.h.state_mut().pad_input(*e);
    }
    let ctx = r.h.ctx.clone();
    let mut raw = std::mem::take(r.h.input_mut());
    r.h.state_mut().pump_gamepad(&ctx, &mut raw);
    *r.h.input_mut() = raw;
    // Enough steps for a page to turn and a cover to fly (about a second).
    r.h.run_steps(8);
}

fn press(r: &mut Rig, b: PadButton) {
    pump(r, &[PadInput::Pressed(b), PadInput::Released(b)]);
}

/// The visible order of the library, as IDs.
fn order(r: &Rig) -> Vec<GameId> {
    let lib = r.h.state().bridge().library().clone();
    let mut vm = r.h.state().state.library.clone();
    vm.visible(&lib)
        .iter()
        .map(|i| lib.games[*i as usize].id)
        .collect()
}

fn focused_card(r: &Rig) -> Option<GameId> {
    let f = r.h.ctx.memory(|m| m.focused())?;
    order(r)
        .into_iter()
        .find(|g| easyplay_ui::screens::library::grid::card_id(*g) == f)
}

#[test]
fn uf15_any_button_enters_controller_mode_on_the_first_card() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadDown);
    assert!(r.h.state().state.controller);
    let first = order(&r)[0];
    assert_eq!(focused_card(&r), Some(first));
    // Footer hints are visible and named.
    r.h.get_by_label_contains("Controller buttons. A: Play, X: Details, Y: Favourite");
    r.h.get_by_label("Collection");
}

#[test]
fn uf15_dpad_moves_a_plays_y_favourites_x_opens() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadRight); // enters controller mode on the first card
    press(&mut r, PadButton::DPadRight);
    let o = order(&r);
    assert_eq!(focused_card(&r), Some(o[1]));
    press(&mut r, PadButton::DPadDown);
    let cols = {
        // The second row starts after one row of cards; find the focused index.
        let f = focused_card(&r).unwrap();
        o.iter().position(|g| *g == f).unwrap() - 1
    };
    assert!(cols >= 2, "down moved one row ({cols} columns)");
    press(&mut r, PadButton::DPadUp);
    assert_eq!(focused_card(&r), Some(o[1]));
    r.fake.clear_sent();
    press(&mut r, PadButton::North);
    press(&mut r, PadButton::South);
    let sent = r.fake.sent();
    assert!(
        sent.iter()
            .any(|c| matches!(c, UiCommand::Core(Command::SetFavorite { id, .. }) if *id == o[1])),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|c| matches!(c, UiCommand::Launch { id, .. } if *id == o[1])),
        "{sent:?}"
    );
    press(&mut r, PadButton::West);
    assert_eq!(r.h.state().state.nav.current(), Route::Game(o[1]));
    // B goes back, and focus returns to the card that was opened.
    press(&mut r, PadButton::East);
    settle(&mut r);
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
    assert_eq!(focused_card(&r), Some(o[1]));
}

#[test]
fn held_dpad_repeats_after_the_delay() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    pump(&mut r, &[PadInput::Pressed(PadButton::DPadRight)]);
    let o = order(&r);
    assert_eq!(focused_card(&r), Some(o[1]));
    // Holding: the harness clock advances with each step; after the delay, repeats follow.
    for _ in 0..40 {
        pump(&mut r, &[]);
    }
    pump(&mut r, &[PadInput::Released(PadButton::DPadRight)]);
    let f = focused_card(&r).unwrap();
    assert!(o.iter().position(|g| *g == f).unwrap() > 1, "repeated");
}

#[test]
fn start_opens_the_menu_and_b_closes_it() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    press(&mut r, PadButton::Start);
    assert!(r.h.state().state.library.menu.is_some());
    press(&mut r, PadButton::East);
    assert!(r.h.state().state.library.menu.is_none());
}

#[test]
fn bumpers_cycle_collections_and_select_focuses_search() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadDown); // enter controller mode
    press(&mut r, PadButton::RightBumper);
    assert_eq!(
        r.h.state().state.library.collection,
        easyplay_core::library::Collection::Favorites
    );
    let first = order(&r)[0];
    assert_eq!(focused_card(&r), Some(first));
    press(&mut r, PadButton::LeftBumper);
    assert_eq!(
        r.h.state().state.library.collection,
        easyplay_core::library::Collection::All
    );
    press(&mut r, PadButton::Select);
    assert!(
        r.h.get_by_role_and_label(Role::TextInput, easyplay_ui::strings::library::SEARCH())
            .accesskit_node()
            .is_focused()
    );
}

#[test]
fn dialogs_answer_to_a_and_b() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    let o = order(&r);
    // Delete asks first; B cancels; focus returns to the card.
    r.h.key_press(Key::Delete);
    r.h.run();
    assert!(r.h.query_all_by_role(Role::Dialog).next().is_some());
    press(&mut r, PadButton::East);
    assert!(r.h.query_all_by_role(Role::Dialog).next().is_none());
    assert_eq!(focused_card(&r), Some(o[0]));
}

#[test]
fn a_running_game_pauses_the_controller_until_focus_returns() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    let o = order(&r);
    r.fake.push_event(UiEvent::LaunchProgress {
        game: o[0],
        state: LaunchState::Running {
            session: easyplay_core::ids::SessionId::from_uuid(uuid::Uuid::from_u128(9)),
        },
    });
    r.h.run();
    // The game's window takes focus; then the user presses a button: ignored.
    r.h.input_mut().focused = false;
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    assert_eq!(focused_card(&r), Some(o[0]), "ignored while unfocused");
    r.h.input_mut().focused = true;
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    assert_eq!(focused_card(&r), Some(o[1]), "works again once focused");
}

#[test]
fn mouse_or_keyboard_input_leaves_controller_mode() {
    let mut r = rig(Route::Library);
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    assert!(r.h.state().state.controller);
    r.h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(600.0, 400.0)));
    pump(&mut r, &[]);
    assert!(!r.h.state().state.controller);
    assert!(r.h.query_by_label_contains("Controller buttons").is_none());
}

#[test]
fn the_preference_turns_controller_navigation_off() {
    let mut r = rig(Route::Library);
    let mut prefs = (**r.h.state().bridge().prefs()).clone();
    prefs.integration.gamepad_navigation = false;
    r.fake.publish_prefs(prefs);
    r.h.run();
    press(&mut r, PadButton::DPadRight);
    assert!(!r.h.state().state.controller);
}

// ---- UX-002: grid focus ---------------------------------------------------------------

#[test]
fn tab_enters_the_grid_at_the_cursor_and_leaves_it_with_the_next_tab() {
    let mut r = rig(Route::Library);
    r.h.run();
    let o = order(&r);
    r.h.get_by_label_contains("Clockwork Garden. ").focus();
    r.h.run();
    r.h.key_press(Key::ArrowRight);
    r.h.run();
    let cursor = focused_card(&r).unwrap();
    assert_ne!(cursor, o[0]);
    r.h.key_press(Key::Tab);
    r.h.run();
    assert_eq!(focused_card(&r), None, "Tab left the grid");
    r.h.key_press_modifiers(egui::Modifiers::SHIFT, Key::Tab);
    r.h.run();
    assert_eq!(
        focused_card(&r),
        Some(cursor),
        "Shift+Tab came back to the cursor"
    );
}

#[test]
fn ctrl_home_and_ctrl_end_reach_the_ends() {
    let mut r = rig(Route::Library);
    r.h.run();
    let o = order(&r);
    r.h.get_by_label_contains("Clockwork Garden. ").focus();
    r.h.run();
    r.h.key_press_modifiers(egui::Modifiers::COMMAND, Key::End);
    r.h.run();
    assert_eq!(focused_card(&r), o.last().copied());
    r.h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Home);
    r.h.run();
    assert_eq!(focused_card(&r), Some(o[0]));
}

#[test]
fn focus_follows_the_game_through_sorting_and_to_a_neighbour_when_filtered_out() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.h.get_by_label_contains("Lantern Keep. ").focus();
    r.h.run();
    let lantern = focused_card(&r).unwrap();
    // Re-sorting keeps focus on the same game.
    r.h.state_mut()
        .state
        .library
        .set_sort(easyplay_core::library::SortKey::Playtime, false);
    r.h.run();
    assert_eq!(focused_card(&r), Some(lantern));
    // Filtering it out moves focus to its nearest neighbour, never nowhere.
    let before = order(&r);
    let at = before.iter().position(|g| *g == lantern).unwrap();
    r.h.state_mut()
        .state
        .library
        .toggle_filter(easyplay_ui::vm::library::FilterChip::Kind(
            easyplay_core::model::EntryKind::Installer,
        ));
    r.h.run();
    let after = order(&r);
    assert!(!after.contains(&lantern));
    let f = focused_card(&r).expect("focus kept in the grid");
    let dist = |g: &GameId| before.iter().position(|x| x == g).unwrap().abs_diff(at);
    let best = after.iter().map(dist).min().unwrap();
    assert_eq!(dist(&f), best, "nearest remaining game");
}
