//! Game page tests (UI-014 … UI-021): rename, favourite, protection editing (UF-6), folder
//! grants (UF-7), launch options rules, launch progress and the problem panel (UF-9, UF-11),
//! the unsaved-changes guard, and snapshots of the page states.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Rig, game_id, now, rebuild, rig_with, settle, settled_library};
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{Activity, Availability, FolderAccess};
use easyplay_ui::bridge::{GameEdit, UiCommand, UiEvent};
use easyplay_ui::demo;
use easyplay_ui::route::Route;
use easyplay_ui::theme::ThemeChoice;
use easyplay_ui::vm::details::DetailsTab;
use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};

fn page(
    name: &str,
    tab: DetailsTab,
    size: Vec2,
    choice: ThemeChoice,
) -> (Rig, easyplay_core::ids::GameId) {
    page_in(settled_library(), name, tab, size, choice)
}

fn page_in(
    lib: easyplay_ui::bridge::LibraryView,
    name: &str,
    tab: DetailsTab,
    size: Vec2,
    choice: ThemeChoice,
) -> (Rig, easyplay_core::ids::GameId) {
    let id = game_id(&lib, name);
    let mut r = rig_with(lib, Route::Game(id), size, choice);
    r.h.state_mut().state.details.tab = tab;
    settle(&mut r);
    (r, id)
}

fn big() -> Vec2 {
    Vec2::new(1280.0, 900.0)
}

fn tall() -> Vec2 {
    Vec2::new(1280.0, 1600.0)
}

#[test]
fn page_renders_from_the_summary_and_fills_in() {
    let (r, _) = page(
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_role_and_label(Role::Button, "Play Lantern Keep");
    r.h.get_all_by_label("What this game can reach")
        .next()
        .unwrap();
    r.h.get_by_label_contains("No internet or local network");
    r.h.get_all_by_label("Notes").next().unwrap();
    assert!(r.h.state().state.details.record().is_some());
}

#[test]
fn f2_renames_inline_with_undo() {
    let (mut r, id) = page(
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
    r.h.key_press(Key::F2);
    r.h.run();
    r.h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    r.h.event(egui::Event::Text("Lantern Keep II".into()));
    r.h.key_press(Key::Enter);
    r.h.run();
    assert!(r.fake.sent().contains(&UiCommand::UpdateGame {
        id,
        edits: vec![GameEdit::Name("Lantern Keep II".into())]
    }));
    r.h.get_by_label("Undo");
}

#[test]
fn protection_edit_shows_the_widening_sentence_and_saves() {
    let (mut r, id) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Share Music").click();
    r.h.run();
    r.h.get_by_label_contains("Custom (based on Offline)");
    r.h.get_by_label_contains("will get this access");
    r.h.get_all_by_label("added").next().unwrap();
    r.h.get_by_role_and_label(Role::Button, "Save").click();
    r.h.run();
    let saved = r.fake.sent().into_iter().find_map(|c| match c {
        UiCommand::UpdateGame { id: g, edits } if g == id => Some(edits),
        _ => None,
    });
    let edits = saved.expect("UpdateGame sent");
    assert!(
        matches!(&edits[0], GameEdit::Policy(p) if p.folders.len() == 1 && p.folders[0].access == FolderAccess::ReadOnly)
    );
}

#[test]
fn a_denied_folder_never_enters_the_list() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    r.picker.answer(vec!["/home/sam".into()]);
    r.h.get_by_label("Add folder…").click();
    r.h.run();
    r.h.run();
    let draft = &r.h.state().state.details.protection.as_ref().unwrap().draft;
    assert!(draft.folders.is_empty());
    r.h.get_by_label_contains("can't be shared: Your whole home folder");
    // An allowed folder appears.
    r.picker.answer(vec!["/home/sam/Mods".into()]);
    r.h.get_by_label("Add folder…").click();
    r.h.run();
    r.h.run();
    let draft = &r.h.state().state.details.protection.as_ref().unwrap().draft;
    assert_eq!(draft.folders.len(), 1);
    r.h.get_all_by_label_contains("Mods, in Home, can read")
        .next()
        .unwrap();
}

#[test]
fn save_is_disabled_with_a_reason_while_errors_exist() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    // Bypass the rejection to put a denied folder in the draft (a stored policy can hold one).
    {
        let s = &mut r.h.state_mut().state;
        let e = s.details.protection.as_mut().unwrap();
        e.add_folder("/home/sam".into(), now());
    }
    r.h.run();
    r.h.run();
    r.h.get_by_label("Fix 1 problem to save");
    let save = r.h.get_by_role_and_label(Role::Button, "Save");
    assert!(save.accesskit_node().is_disabled());
}

#[test]
fn leaving_with_unsaved_changes_asks_and_keeps_editing_by_default() {
    let (mut r, id) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Share Music").click();
    r.h.run();
    r.h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Discard your changes?");
    r.h.key_press(Key::Enter);
    r.h.run();
    assert_eq!(r.h.state().state.nav.current(), Route::Game(id));
    assert!(r.h.state().state.details.dirty());
}

#[test]
fn reserved_variables_cannot_be_saved() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::LaunchOptions,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label_contains("Advanced").click();
    r.h.run();
    r.h.get_by_label("Add variable").click();
    r.h.run();
    r.h.get_by_label("Variable name, row 2").focus();
    r.h.get_by_label("Variable name, row 2").type_text("PATH");
    r.h.run();
    r.h.get_by_label_contains("PATH is managed by EasyPlay");
    assert!(
        r.h.get_by_role_and_label(Role::Button, "Save")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn launch_progress_cancels_and_failures_show_the_problem_panel() {
    let (mut r, id) = page(
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
    r.fake.push_event(UiEvent::LaunchProgress {
        game: id,
        state: LaunchState::Validating,
    });
    r.h.run_steps(2);
    r.h.get_all_by_label("Checking…").next().unwrap();
    r.h.get_by_role_and_label(Role::Button, "Cancel").click();
    r.h.run_steps(2);
    assert!(r.fake.sent().contains(&UiCommand::CancelLaunch { id }));
    r.fake.push_event(UiEvent::LaunchProgress {
        game: id,
        state: LaunchState::Failed(Box::new(demo::launch_failure(id, "Lantern Keep"))),
    });
    r.h.run_steps(2);
    r.h.get_all_by_label("This game needs a folder that isn't there")
        .next()
        .unwrap();
    r.h.get_by_role_and_label(Role::Button, "Edit permissions")
        .click();
    r.h.run_steps(2);
    assert_eq!(r.h.state().state.details.tab, DetailsTab::Protection);
}

#[test]
fn compatibility_tab_measures_storage_and_offers_reset() {
    let (mut r, id) = page(
        "Lantern Keep",
        DetailsTab::Compatibility,
        tall(),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    r.h.get_by_label_contains("2.3 GB");
    r.h.get_by_label("Reset…").click();
    r.h.run();
    r.h.get_by_role_and_label(
        Role::Dialog,
        "Reset private Windows storage for Lantern Keep?",
    );
    r.h.key_press(Key::Escape);
    r.h.run();
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::ResetPrefix { id: g, .. } if *g == id))
    );
}

// ---- Snapshots ---------------------------------------------------------------------------

fn snap(name: &str, game: &str, tab: DetailsTab, size: Vec2, choice: ThemeChoice) {
    let (mut r, _) = page(game, tab, size, choice);
    r.h.run_steps(4);
    r.h.snapshot(name);
}

#[test]
fn snapshot_page_overview_dark() {
    snap(
        "game_overview_dark_1280x900",
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
}

#[test]
fn snapshot_page_overview_light() {
    snap(
        "game_overview_light_1280x900",
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Light,
    );
}

#[test]
fn snapshot_page_overview_compact() {
    snap(
        "game_overview_dark_640x480",
        "Lantern Keep",
        DetailsTab::Overview,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
}

#[test]
fn snapshot_page_protection_dark() {
    let (mut r, _) = page(
        "Hollow Bell",
        DetailsTab::Protection,
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Share Pictures").click();
    r.h.run_steps(4);
    r.h.snapshot("game_protection_dark_1280x1400");
}

#[test]
fn snapshot_page_protection_light() {
    snap(
        "game_protection_light_1280x1400",
        "Hollow Bell",
        DetailsTab::Protection,
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Light,
    );
}

#[test]
fn snapshot_page_compatibility_dark() {
    snap(
        "game_compat_dark_1280x1100",
        "Lantern Keep",
        DetailsTab::Compatibility,
        Vec2::new(1280.0, 1100.0),
        ThemeChoice::Dark,
    );
}

#[test]
fn snapshot_page_launch_options_dark() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::LaunchOptions,
        Vec2::new(1280.0, 1000.0),
        ThemeChoice::Dark,
    );
    r.h.get_by_label_contains("Advanced").click();
    r.h.run_steps(4);
    r.h.snapshot("game_launch_dark_1280x1000");
}

#[test]
fn snapshot_page_activity_dark() {
    snap(
        "game_activity_dark_1280x1100",
        "Amber Sky",
        DetailsTab::Activity,
        Vec2::new(1280.0, 1100.0),
        ThemeChoice::Dark,
    );
}

fn odd_states() -> easyplay_ui::bridge::LibraryView {
    rebuild(&settled_library(), |g| {
        if &*g.name == "Paper Kingdoms" {
            g.availability = Availability::Missing;
        }
        if &*g.name == "Starfall Tactics" {
            g.activity = Activity::Lingering;
        }
    })
}

#[test]
fn snapshot_page_missing() {
    let (mut r, _) = page_in(
        odd_states(),
        "Paper Kingdoms",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
    r.h.run_steps(4);
    r.h.snapshot("game_missing_dark_1280x900");
}

#[test]
fn snapshot_page_lingering() {
    let (mut r, _) = page_in(
        odd_states(),
        "Starfall Tactics",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Light,
    );
    r.h.run_steps(4);
    r.h.snapshot("game_lingering_light_1280x900");
}

#[test]
fn snapshot_page_problem_panel() {
    let (mut r, id) = page("Tidewater", DetailsTab::Overview, big(), ThemeChoice::Dark);
    r.fake.push_event(UiEvent::LaunchProgress {
        game: id,
        state: LaunchState::Failed(Box::new(demo::launch_failure(id, "Tidewater"))),
    });
    r.h.run_steps(4);
    r.h.snapshot("game_problem_dark_1280x900");
    let _ = now();
}
