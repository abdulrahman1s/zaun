//! Game page tests (UI-014 … UI-021): rename, favourite, protection editing (UF-6), folder
//! grants (UF-7), launch options rules, launch progress and the problem panel (UF-9, UF-11),
//! the unsaved-changes guard, and snapshots of the page states.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Rig, game_id, now, rebuild, rig_with, settle, settled_library};
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{Activity, Availability, FolderAccess};
use easyplay_ui::bridge::{FolderTarget, GameEdit, QueryPayload, UiCommand, UiEvent};
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

/// Flip a permission's switch on the Protection tab.
fn flip(r: &mut Rig, permission: &str) {
    r.h.get_by_role_and_label(Role::Switch, permission).click();
    r.h.run();
}

/// Open the degrees of a permission and take one.
fn degree(r: &mut Rig, control: &str, choice: &str) {
    r.h.get_by_role_and_label(Role::ComboBox, control).click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, choice).click();
    r.h.run();
}

/// What a control says after its name.
fn described(r: &Rig, label: &str) -> String {
    r.h.get_by_label(label)
        .accesskit_node()
        .description()
        .unwrap_or_default()
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
    r.h.get_by_role_and_label(Role::Button, "Protection: Offline");
    r.h.get_by_role_and_label(Role::List, "What this game can reach");
    r.h.get_by_role_and_label(Role::ListItem, "No internet or local network");
    r.h.get_all_by_label("Notes").next().unwrap();
    assert!(r.h.state().state.details.record().is_some());
}

#[test]
fn the_facts_of_play_stand_beside_play() {
    let (r, _) = page(
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Last played: Yesterday");
    r.h.get_by_label("Time played: 12 h 05 min");
    r.h.get_by_label("Last 7 days: 1 hour 12 minutes");
    // The facts are on every tab, and a game never played says so instead.
    let (r, _) = page(
        "Vale of Cinders",
        DetailsTab::Protection,
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Not played yet");
    assert!(r.h.query_by_label_contains("Time played").is_none());
    // A narrow window keeps the facts that fit and drops the week first.
    let (r, _) = page(
        "Lantern Keep",
        DetailsTab::Overview,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Time played: 12 h 05 min");
    assert!(r.h.query_by_label_contains("Last 7 days").is_none());
}

#[test]
fn the_week_is_asked_again_when_a_session_was_counted() {
    let lib = settled_library();
    let (mut r, id) = page_in(
        lib.clone(),
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
    let asked = |r: &Rig| {
        r.fake
            .sent()
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    UiCommand::QueryStats {
                        request: easyplay_core::stats::StatsRequest::GameOverview { game, .. },
                        ..
                    } if *game == id
                )
            })
            .count()
    };
    r.h.run_steps(4);
    assert_eq!(asked(&r), 0, "nothing is asked again while nothing changed");
    r.fake.publish_library(rebuild(&lib, |g| {
        if g.id == id {
            g.session_count += 1;
            g.total_playtime_ms += 30 * 60_000;
        }
    }));
    r.h.run_steps(3);
    assert_eq!(asked(&r), 1);
    // The week on screen stays until its successor arrives.
    r.h.get_by_label("Last 7 days: 1 hour 12 minutes");
}

#[test]
fn overview_rows_lead_to_their_tabs() {
    for (row, tab) in [
        ("Protection: Offline", DetailsTab::Protection),
        (
            "Compatibility: UMU-Proton 10.0-4",
            DetailsTab::Compatibility,
        ),
        (
            "Launch options: LanternKeep.exe, -screen-fullscreen 1 · 1 variable",
            DetailsTab::LaunchOptions,
        ),
    ] {
        let (mut r, _) = page(
            "Lantern Keep",
            DetailsTab::Overview,
            tall(),
            ThemeChoice::Dark,
        );
        r.h.get_by_role_and_label(Role::Button, row).click();
        r.h.run_steps(2);
        assert_eq!(r.h.state().state.details.tab, tab, "{row}");
    }
    // The keyboard does the same: the row is a button.
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::Overview,
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Compatibility: UMU-Proton 10.0-4").focus();
    r.h.run();
    r.h.key_press(Key::Enter);
    r.h.run_steps(2);
    assert_eq!(r.h.state().state.details.tab, DetailsTab::Compatibility);
}

#[test]
fn overview_rows_open_the_folders_and_run_the_other_programs() {
    let (mut r, id) = page(
        "Lantern Keep",
        DetailsTab::Overview,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Game folder: Lantern Keep, in Games")
        .click();
    r.h.run_steps(2);
    r.h.get_by_label("Save data: Private storage").click();
    r.h.run_steps(2);
    r.h.get_by_role_and_label(Role::Button, "Run Config.exe")
        .click();
    r.h.run_steps(2);
    let sent = r.fake.sent();
    assert!(sent.contains(&UiCommand::OpenFolder(FolderTarget::GameFolder(id))));
    assert!(sent.contains(&UiCommand::OpenFolder(FolderTarget::SaveData(id))));
    assert!(
        sent.iter()
            .any(|c| matches!(c, UiCommand::Launch { id: g, exe: Some(_) } if *g == id))
    );
}

#[test]
fn a_record_that_fails_to_load_offers_retry() {
    let lib = settled_library();
    let id = game_id(&lib, "Lantern Keep");
    let mut r = rig_with(lib, Route::Game(id), big(), ThemeChoice::Dark);
    r.h.run_steps(2);
    let load = |r: &Rig| {
        r.fake.sent().into_iter().find_map(|c| match c {
            UiCommand::LoadGame { reply_to, .. } => Some(reply_to),
            _ => None,
        })
    };
    let q = load(&r).expect("the record is asked for");
    r.fake.clear_sent();
    r.fake.push_event(UiEvent::QueryResult {
        id: q,
        payload: QueryPayload::Failed(demo::launch_failure(id, "Lantern Keep")),
    });
    r.h.run_steps(2);
    // The page still stands on the summary; the sheet says what is missing.
    r.h.get_by_role_and_label(Role::Button, "Play Lantern Keep");
    r.h.get_by_label("Couldn't load this.");
    r.h.get_by_role_and_label(Role::Button, "Retry").click();
    r.h.run_steps(2);
    assert!(load(&r).is_some());
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
    flip(&mut r, "Music");
    r.h.get_by_label_contains("Custom (based on Offline)");
    r.h.get_by_label_contains("will get this access");
    // The switch is on and marked until it is saved; the folder may be read.
    assert_eq!(described(&r, "Music"), "changed");
    r.h.get_by_role_and_label(Role::ComboBox, "Music: Can read");
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
    assert_eq!(described(&r, "Mods"), "in Home. changed");
}

#[test]
fn the_microphone_is_a_switch_of_its_own_that_needs_sound() {
    use easyplay_core::model::{AudioAccess, Permissions};
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    let audio = |r: &Rig| {
        let e = r.h.state().state.details.protection.as_ref().unwrap();
        e.draft.devices.audio
    };
    // Sound is on and the microphone off by default.
    assert_eq!(audio(&r), AudioAccess::Playback);
    flip(&mut r, "Microphone");
    assert_eq!(audio(&r), AudioAccess::Full);
    // Sound off takes the microphone with it; the microphone on brings sound back.
    flip(&mut r, "Sound");
    assert_eq!(audio(&r), AudioAccess::None);
    flip(&mut r, "Microphone");
    assert_eq!(audio(&r), AudioAccess::Full);
    let e = r.h.state().state.details.protection.as_ref().unwrap();
    assert!(
        e.draft
            .permissions()
            .contains(Permissions::SOUND | Permissions::MICROPHONE)
    );
}

#[test]
fn every_permission_is_a_switch_and_some_have_degrees() {
    use easyplay_core::model::{ControllerAccess, GameDirAccess, Permissions, RemovableAccess};
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    let draft = |r: &Rig| {
        let e = r.h.state().state.details.protection.as_ref().unwrap();
        (e.draft.clone(), e.dirty())
    };
    let offline = Permissions::SOUND | Permissions::CONTROLLERS;
    assert_eq!(draft(&r).0.permissions(), offline);
    flip(&mut r, "Internet and local network");
    flip(&mut r, "Camera");
    flip(&mut r, "Sound");
    flip(&mut r, "Removable drives");
    let (d, dirty) = draft(&r);
    assert!(dirty);
    assert_eq!(
        d.permissions(),
        Permissions::INTERNET
            | Permissions::CAMERA
            | Permissions::CONTROLLERS
            | Permissions::REMOVABLE
    );
    // A switch that is on shows how far it goes; the degree is a choice of its own.
    degree(&mut r, "Removable drives: Can read", "Can read and change");
    degree(
        &mut r,
        "Game controllers: Connected when the game starts",
        "All input devices",
    );
    degree(&mut r, "Game folder: Can read and change", "Can read");
    let (d, _) = draft(&r);
    assert_eq!(d.removable, RemovableAccess::ReadWrite);
    assert_eq!(d.devices.controllers, ControllerAccess::AllInput);
    assert_eq!(d.game_dir, GameDirAccess::ReadOnly);
    r.h.get_by_role_and_label(Role::ComboBox, "Game controllers: All input devices");
    // Switched off and on again, a permission comes back at the degree that is saved.
    flip(&mut r, "Game controllers");
    assert_eq!(draft(&r).0.devices.controllers, ControllerAccess::None);
    flip(&mut r, "Game controllers");
    assert_eq!(draft(&r).0.devices.controllers, ControllerAccess::Detected);
    // Taking everything back leaves nothing to save.
    flip(&mut r, "Internet and local network");
    flip(&mut r, "Camera");
    flip(&mut r, "Sound");
    flip(&mut r, "Removable drives");
    degree(&mut r, "Game folder: Can read", "Can read and change");
    let (d, dirty) = draft(&r);
    assert_eq!(d.permissions(), offline);
    assert!(!dirty);
}

#[test]
fn a_folder_that_is_switched_off_keeps_its_row_until_saved() {
    let (mut r, _) = page(
        "Hollow Bell",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    let dirty = |r: &Rig| r.h.state().state.details.dirty();
    // Hollow Bell shares Music. Its switch goes off where it stands, marked.
    flip(&mut r, "Music");
    assert!(dirty(&r));
    assert_eq!(described(&r, "Music"), "changed");
    // On again, it is as it was saved: no change at all.
    flip(&mut r, "Music");
    assert!(!dirty(&r));
    // A folder of the user's choosing has a row only while it is shared; switched off, the
    // row waits until the change is saved.
    r.picker.answer(vec!["/home/sam/Mods".into()]);
    r.h.get_by_label("Add folder…").click();
    r.h.run();
    r.h.run();
    degree(&mut r, "Mods: Can read", "Can read and change");
    r.h.get_by_role_and_label(Role::Button, "Save").click();
    r.h.run();
    assert!(!dirty(&r));
    r.h.get_by_role_and_label(Role::Switch, "Mods").focus();
    r.h.run();
    r.h.key_press(Key::Delete);
    r.h.run();
    assert!(dirty(&r));
    flip(&mut r, "Mods");
    assert!(!dirty(&r));
    r.h.get_by_role_and_label(Role::ComboBox, "Mods: Can read and change");
}

#[test]
fn what_advanced_opens_up_is_listed_and_leads_there() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_role_and_label(Role::RadioButton, "Standard")
        .click();
    r.h.run();
    let advanced = |r: &Rig| {
        let e = r.h.state().state.details.protection.as_ref().unwrap();
        e.advanced
    };
    assert!(!advanced(&r));
    // Standard shows the computer's identity: said among the allowed things, in the
    // statement's own words, and decided under Advanced.
    r.h.get_by_role_and_label(Role::Button, "Can see this computer's name and ID")
        .click();
    r.h.run();
    assert!(advanced(&r));
    r.h.get_by_label("Hide the machine ID");
}

#[test]
fn a_choice_that_cannot_be_made_says_why() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::Protection,
        tall(),
        ThemeChoice::Dark,
    );
    let mut host = demo::host_view();
    host.input_readable = true;
    host.x11 = true;
    r.fake.publish_host(host);
    settle(&mut r);
    // On an X11 desktop the page says what that means for every game.
    assert!(described(&r, "Screen").contains("not isolated"));
    r.h.get_by_role_and_label(
        Role::ComboBox,
        "Game controllers: Connected when the game starts",
    )
    .click();
    r.h.run();
    let all =
        r.h.get_by_role_and_label(Role::MenuItem, "All input devices");
    assert!(all.accesskit_node().is_disabled());
    assert!(
        all.accesskit_node()
            .description()
            .unwrap_or_default()
            .contains("can read keyboards directly")
    );
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
    flip(&mut r, "Music");
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
fn the_overview_says_what_the_files_tell() {
    let (r, _) = page(
        "Hollow Bell",
        DetailsTab::Overview,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Engine: Unreal Engine");
    r.h.get_by_label_contains("Anti-cheat: Easy Anti-Cheat: often refuses to run");
    r.h.get_by_label("Online services: Uses Steam's services");
    r.h.get_by_label("Program: 64-bit Windows program");
}

#[test]
fn a_variable_name_suggests_proton_options_with_what_they_do() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::LaunchOptions,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label_contains("Advanced").click();
    r.h.run();
    r.h.get_by_label("Add variable").click();
    r.h.run_steps(2);
    // The new name has the focus, and the options are offered at once.
    r.h.get_by_label("Variable name, row 2").type_text("frame");
    r.h.run_steps(2);
    r.h.get_by_role_and_label(
        Role::ListBoxOption,
        "DXVK_FRAME_RATE: Cap the frame rate of Direct3D 9–11 games",
    );
    // Down to the second, and Enter takes it with its usual value.
    r.h.key_press(Key::ArrowDown);
    r.h.run();
    r.h.key_press(Key::Enter);
    r.h.run_steps(2);
    let l = r.h.state().state.details.launch.as_ref().unwrap();
    let v = l.config.env.last().unwrap();
    assert_eq!(
        (v.name.as_str(), v.value.as_str()),
        ("VKD3D_FRAME_RATE", "60")
    );
    // What it does is said under its row.
    r.h.get_by_label("Cap the frame rate of Direct3D 12 games");
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

/// The searches sent since the last call, each answered as the service answers a query it
/// cannot run.
fn fail_fix_searches(r: &mut Rig, id: easyplay_core::ids::GameId) -> usize {
    let sent = r.fake.sent();
    r.fake.clear_sent();
    let mut n = 0;
    for c in sent {
        if let UiCommand::SearchFixes { reply_to, .. } = c {
            n += 1;
            r.fake.push_event(UiEvent::QueryResult {
                id: reply_to,
                payload: QueryPayload::Failed(demo::launch_failure(id, "Lantern Keep")),
            });
        }
    }
    n
}

/// A search that cannot be answered is still an answer: it is shown, the field keeps what
/// is typed, and the search is not sent again with the game's name on every frame.
#[test]
fn a_fix_search_that_fails_is_shown_and_not_repeated() {
    let (mut r, id) = page(
        "Lantern Keep",
        DetailsTab::Compatibility,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.state_mut()
        .state
        .nav
        .open(easyplay_ui::route::Overlay::FixPicker(id));
    r.h.run_steps(2);
    assert_eq!(fail_fix_searches(&mut r, id), 1);
    r.h.run_steps(4);
    r.h.get_by_label_contains("that folder can't be found");
    assert_eq!(fail_fix_searches(&mut r, id), 0);
    let field = r.h.get_by_label("Search by title, store or ID");
    field.focus();
    field.type_text(" 2");
    r.h.run_steps(2);
    assert!(fail_fix_searches(&mut r, id) >= 1);
    r.h.run_steps(4);
    assert_eq!(r.h.state().state.details.fix_search, "Lantern Keep 2");
    assert_eq!(fail_fix_searches(&mut r, id), 0);
}

/// A failed page of the session history is not asked for again on every frame.
#[test]
fn a_failed_history_page_is_not_requested_every_frame() {
    let lib = settled_library();
    let id = lib
        .games
        .iter()
        .find(|g| g.session_count > 0)
        .expect("a played sample game")
        .id;
    let mut r = rig_with(lib, Route::Game(id), big(), ThemeChoice::Dark);
    r.h.state_mut().state.details.tab = DetailsTab::Activity;
    let mut asked = 0;
    for _ in 0..6 {
        r.h.run_steps(2);
        let sent = r.fake.sent();
        r.fake.clear_sent();
        for c in sent {
            let UiCommand::QueryStats { request, reply_to } = c else {
                continue;
            };
            if matches!(
                request,
                easyplay_core::stats::StatsRequest::SessionHistory { .. }
            ) {
                asked += 1;
            }
            r.fake.push_event(UiEvent::QueryResult {
                id: reply_to,
                payload: QueryPayload::Failed(demo::launch_failure(id, "X")),
            });
        }
    }
    assert_eq!(asked, 1);
    assert!(r.h.state().state.details.activity.history_failed);
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
    flip(&mut r, "Pictures");
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
fn snapshot_launch_options_suggestions_dark() {
    let (mut r, _) = page(
        "Lantern Keep",
        DetailsTab::LaunchOptions,
        tall(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label_contains("Advanced").click();
    r.h.run();
    r.h.get_by_label("Add variable").click();
    r.h.run_steps(2);
    r.h.get_by_label("Variable name, row 2")
        .type_text("PROTON_");
    r.h.run_steps(4);
    r.h.snapshot("game_launch_suggestions_dark_1280x1600");
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
