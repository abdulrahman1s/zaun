//! A11Y-003: keyboard-only walkthroughs of the main flows (USER_FLOWS UF-1 … UF-14) and
//! focus return after every overlay (ACCESSIBILITY §2, §8). No test here sends a pointer
//! event: every step is a key press or typed text; the "system" side (service events) is
//! simulated through the fake backend.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use common::{Rig, game_id, now, rebuild, rig, rig_on, settle, settled_library};
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{Activity, Availability, PromptKind, PromptRequest};
use easyplay_ui::bridge::{FakeBackend, SetupContext, SetupOrigin, UiCommand, UiEvent};
use easyplay_ui::demo;
use easyplay_ui::input::picker::PickPurpose;
use easyplay_ui::route::Route;
use easyplay_ui::theme::ThemeChoice;
use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};

/// The label of the focused node (`None`: nothing focused).
fn focused(r: &Rig) -> Option<String> {
    r.h.query_by(|n| n.is_focused())
        .map(|n| n.accesskit_node().label().unwrap_or_default())
}

fn key(r: &mut Rig, k: Key) {
    r.h.key_press(k);
    r.h.run();
}

fn key_mod(r: &mut Rig, m: Modifiers, k: Key) {
    r.h.key_press_modifiers(m, k);
    r.h.run();
}

fn type_text(r: &mut Rig, text: &str) {
    r.h.input_mut().events.push(egui::Event::Text(text.into()));
    r.h.run();
}

/// Press `Tab` (or `Shift+Tab`) until the focused control's name contains `label`.
fn tab_to(r: &mut Rig, label: &str) {
    tab_to_dir(r, label, false);
}

fn tab_to_dir(r: &mut Rig, label: &str, back: bool) {
    let mut seen = Vec::new();
    for _ in 0..80 {
        if focused(r).is_some_and(|f| f.contains(label)) {
            return;
        }
        let m = if back {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        key_mod(r, m, Key::Tab);
        seen.push(focused(r).unwrap_or_default());
    }
    panic!("no control named {label:?} reachable with Tab; visited {seen:#?}");
}

/// Open a game's page from the library with the keyboard: type to search, `Tab` to the
/// card, `Enter`.
fn open_game(r: &mut Rig, name: &str) -> easyplay_core::ids::GameId {
    type_text(r, name);
    tab_to(r, &format!("{name}. "));
    key(r, Key::Enter);
    settle(r);
    match r.h.state().state.nav.current() {
        Route::Game(id) => id,
        other => panic!("expected the game page, got {other:?}"),
    }
}

fn sent(r: &Rig) -> Vec<UiCommand> {
    r.fake.sent()
}

fn picks(r: &Rig) -> Vec<PickPurpose> {
    r.picker.asked()
}

// ---- UF-1 First start --------------------------------------------------------------------

#[test]
fn uf1_onboarding_by_keyboard() {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.onboarding.completed = false;
    fake.publish_prefs(prefs);
    fake.publish_host(demo::host_view());
    let mut r = rig_on(
        fake,
        Route::Onboarding,
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    for _ in 0..3 {
        tab_to(&mut r, "Continue");
        key(&mut r, Key::Enter);
    }
    tab_to(&mut r, "Start using EasyPlay");
    key(&mut r, Key::Space);
    assert!(sent(&r).contains(&UiCommand::CompleteOnboarding { skipped: false }));
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

// ---- UF-2 / UF-4 Setup ------------------------------------------------------------------

fn request_setup(r: &Rig, origin: SetupOrigin) {
    r.fake.push_event(UiEvent::SetupRequested {
        draft: Box::new(demo::draft(
            Path::new("/home/sam/Games/Celeste/Celeste.exe"),
            now(),
        )),
        context: SetupContext {
            origin,
            queue: (1, 1),
        },
    });
}

#[test]
fn uf2_setup_choose_online_and_play_by_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    request_setup(&r, SetupOrigin::FileManager);
    r.h.run();
    // The preset selector: Tab to it, arrows choose.
    // The preset selector: Tab to the option, Space chooses it.
    tab_to(&mut r, "Online");
    key(&mut r, Key::Space);
    r.h.get_by_label_contains("Can use the internet");
    tab_to(&mut r, "Play");
    key(&mut r, Key::Enter);
    let created = sent(&r).into_iter().find_map(|c| match c {
        UiCommand::CreateGame { draft, .. } => Some(draft.policy.network),
        _ => None,
    });
    assert_eq!(
        created,
        Some(easyplay_core::model::NetworkAccess::Full),
        "{:?}",
        sent(&r)
    );
}

#[test]
fn uf4_add_game_with_ctrl_n_then_cancel_setup_with_escape() {
    let mut r = rig(Route::Library);
    r.h.run();
    key_mod(&mut r, Modifiers::COMMAND, Key::N);
    assert!(picks(&r).contains(&PickPurpose::AddGame));
    request_setup(&r, SetupOrigin::InsideApp);
    r.h.run();
    key(&mut r, Key::Escape);
    assert!(r.h.state().state.nav.overlays().is_empty());
    assert!(
        !sent(&r)
            .iter()
            .any(|c| matches!(c, UiCommand::CreateGame { .. }))
    );
}

// ---- UF-3 Quick launch ------------------------------------------------------------------

#[test]
fn uf3_quick_launch_opens_easyplay_by_keyboard() {
    let fake = FakeBackend::new(settled_library());
    let mut r = common::rig_app(
        fake,
        Route::Library,
        Vec2::new(420.0, 150.0),
        ThemeChoice::Dark,
        |a| a.quick_launch("/home/sam/Games/Lantern Keep/LanternKeep.exe".into()),
    );
    r.h.run_steps(3);
    for _ in 0..10 {
        if focused(&r).is_some_and(|f| f.contains("Open EasyPlay")) {
            break;
        }
        r.h.key_press(Key::Tab);
        r.h.run_steps(2);
    }
    r.h.key_press(Key::Enter);
    r.h.run_steps(2);
    assert!(r.h.state().state.quick.as_ref().is_none_or(|q| q.expanded));
}

// ---- UF-5 Installer prompt -------------------------------------------------------------

#[test]
fn uf5_installer_prompt_answers_with_enter() {
    let mut r = rig(Route::Library);
    r.h.run();
    r.fake.push_event(UiEvent::Prompt(PromptRequest {
        id: easyplay_core::ids::PromptId::from_uuid(uuid::Uuid::from_u128(5)),
        kind: PromptKind::InstallerFinished {
            game: game_id(&settled_library(), "Setup Glassworks"),
            programs: vec![easyplay_core::model::ProgramCandidate {
                path: "/p/Glassworks.exe".into(),
                label: "Glassworks".into(),
                from_shortcut: true,
                size: 48_000_000,
            }],
        },
    }));
    r.h.run();
    key(&mut r, Key::Enter);
    assert!(
        sent(&r)
            .iter()
            .any(|c| matches!(c, UiCommand::AnswerPrompt { .. })),
        "{:?}",
        sent(&r)
    );
}

// ---- UF-6 / UF-8 Changing protection ---------------------------------------------------

#[test]
fn uf6_uf8_switch_to_online_save_and_play_by_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    let id = open_game(&mut r, "Lantern Keep");
    // Ctrl+Tab to the Protection tab.
    while r.h.state().state.details.tab != easyplay_ui::vm::details::DetailsTab::Protection {
        key_mod(&mut r, Modifiers::COMMAND, Key::Tab);
    }
    settle(&mut r);
    tab_to(&mut r, "Online");
    key(&mut r, Key::Space);
    tab_to(&mut r, "Save");
    key(&mut r, Key::Enter);
    settle(&mut r);
    assert!(
        sent(&r)
            .iter()
            .any(|c| matches!(c, UiCommand::UpdateGame { id: g, .. } if *g == id)),
        "{:?}",
        sent(&r)
    );
    key_mod(&mut r, Modifiers::COMMAND, Key::Enter);
    assert!(
        sent(&r)
            .iter()
            .any(|c| matches!(c, UiCommand::Launch { id: g, .. } if *g == id))
    );
}

// ---- UF-7 Sharing a folder -------------------------------------------------------------

#[test]
fn uf7_add_folder_opens_the_picker_by_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    let id = open_game(&mut r, "Lantern Keep");
    while r.h.state().state.details.tab != easyplay_ui::vm::details::DetailsTab::Protection {
        key_mod(&mut r, Modifiers::COMMAND, Key::Tab);
    }
    settle(&mut r);
    tab_to(&mut r, "Add folder");
    key(&mut r, Key::Enter);
    assert!(picks(&r).contains(&PickPurpose::FolderGrant(Some(id))));
}

#[test]
fn uf7_a_permission_is_changed_by_keyboard() {
    use easyplay_core::model::FolderAccess;
    let mut r = rig(Route::Library);
    r.h.run();
    open_game(&mut r, "Lantern Keep");
    while r.h.state().state.details.tab != easyplay_ui::vm::details::DetailsTab::Protection {
        key_mod(&mut r, Modifiers::COMMAND, Key::Tab);
    }
    settle(&mut r);
    let access = |r: &Rig| {
        let draft = &r.h.state().state.details.protection.as_ref().unwrap().draft;
        draft.folders.first().map(|f| f.access)
    };
    // Space flips the switch; the row keeps its place and the focus.
    tab_to(&mut r, "Music");
    key(&mut r, Key::Space);
    assert_eq!(access(&r), Some(FolderAccess::ReadOnly));
    assert_eq!(focused(&r).as_deref(), Some("Music"));
    // Its degree is the next stop: Enter opens the choices, the arrows move among them,
    // Enter takes one and the focus returns to the control.
    key(&mut r, Key::Tab);
    assert_eq!(focused(&r).as_deref(), Some("Music: Can read"));
    key(&mut r, Key::Enter);
    key(&mut r, Key::ArrowDown);
    key(&mut r, Key::ArrowDown);
    assert_eq!(focused(&r).as_deref(), Some("Can read and change"));
    key(&mut r, Key::Enter);
    assert_eq!(access(&r), Some(FolderAccess::ReadWrite));
    assert_eq!(focused(&r).as_deref(), Some("Music: Can read and change"));
}

// ---- UF-9 Problem panel ----------------------------------------------------------------

#[test]
fn uf9_try_again_from_the_problem_panel_by_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    let id = open_game(&mut r, "Lantern Keep");
    r.fake.push_event(UiEvent::LaunchProgress {
        game: id,
        state: LaunchState::Failed(Box::new(demo::launch_failure(id, "Lantern Keep"))),
    });
    settle(&mut r);
    r.fake.clear_sent();
    tab_to(&mut r, "Try again");
    key(&mut r, Key::Enter);
    assert!(
        sent(&r)
            .iter()
            .any(|c| matches!(c, UiCommand::Launch { id: g, .. } if *g == id)),
        "{:?}",
        sent(&r)
    );
}

// ---- UF-10 Missing game -----------------------------------------------------------------

#[test]
fn uf10_locate_a_missing_game_by_keyboard() {
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Lantern Keep" {
            g.availability = Availability::Missing;
        }
    });
    let mut r = common::rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    let id = open_game(&mut r, "Lantern Keep");
    tab_to(&mut r, "Locate");
    key(&mut r, Key::Enter);
    assert!(picks(&r).contains(&PickPurpose::Locate(id)));
}

// ---- UF-11 Stopping --------------------------------------------------------------------

#[test]
fn uf11_ctrl_period_stops_the_running_game() {
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Lantern Keep" {
            g.activity = Activity::Running { since: now() };
        }
    });
    let mut r = common::rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    let id = open_game(&mut r, "Lantern Keep");
    key_mod(&mut r, Modifiers::COMMAND, Key::Period);
    assert!(sent(&r).contains(&UiCommand::Stop { id, force: false }));
}

// ---- UF-12 Removing ----------------------------------------------------------------------

#[test]
fn uf12_remove_from_the_library_by_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    type_text(&mut r, "Lantern Keep");
    tab_to(&mut r, "Lantern Keep. ");
    key(&mut r, Key::Delete);
    assert!(r.h.query_all_by_role(Role::Dialog).next().is_some());
    // The safe choice is the default: Enter alone would cancel; Tab to the danger button.
    tab_to(&mut r, "Remove game");
    key(&mut r, Key::Enter);
    let id = game_id(&settled_library(), "Lantern Keep");
    assert!(
        sent(&r)
            .iter()
            .any(|c| matches!(c, UiCommand::RemoveGame { id: g, .. } if *g == id)),
        "{:?}",
        sent(&r)
    );
}

// ---- UF-13 Resetting storage -------------------------------------------------------------

#[test]
fn uf13_reset_private_storage_by_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    let id = open_game(&mut r, "Lantern Keep");
    while r.h.state().state.details.tab != easyplay_ui::vm::details::DetailsTab::Compatibility {
        key_mod(&mut r, Modifiers::COMMAND, Key::Tab);
    }
    settle(&mut r);
    tab_to(&mut r, "Reset…");
    key(&mut r, Key::Enter);
    assert!(r.h.query_all_by_role(Role::Dialog).next().is_some());
    tab_to(&mut r, "Reset");
    key(&mut r, Key::Enter);
    assert!(
        sent(&r)
            .iter()
            .any(|c| matches!(c, UiCommand::ResetPrefix { id: g, .. } if *g == id)),
        "{:?}",
        sent(&r)
    );
}

// ---- UF-14 Compatibility layers ----------------------------------------------------------

#[test]
fn uf14_settings_layers_check_for_updates_by_keyboard() {
    let mut r = rig(Route::Library);
    r.h.run();
    key_mod(&mut r, Modifiers::COMMAND, Key::Comma);
    assert!(matches!(
        r.h.state().state.nav.current(),
        Route::Settings(_)
    ));
    while r.h.state().state.nav.current()
        != Route::Settings(easyplay_ui::route::SettingsTab::CompatibilityLayers)
    {
        key_mod(&mut r, Modifiers::COMMAND, Key::Tab);
    }
    settle(&mut r);
    r.fake.clear_sent();
    tab_to(&mut r, "Check for updates");
    key(&mut r, Key::Enter);
    assert!(!sent(&r).is_empty(), "a command was sent");
    // Back to the library with Escape.
    key(&mut r, Key::Escape);
}

// ---- Focus return after overlays ---------------------------------------------------------

/// Focus a card, open an overlay with `open`, close it with `Esc`: focus is on the card.
fn returns_to_card(open: impl Fn(&mut Rig)) {
    let mut r = rig(Route::Library);
    r.h.run();
    type_text(&mut r, "Lantern Keep");
    tab_to(&mut r, "Lantern Keep. ");
    let before = focused(&r).unwrap();
    open(&mut r);
    assert_ne!(
        focused(&r).as_deref(),
        Some(before.as_str()),
        "overlay took focus"
    );
    key(&mut r, Key::Escape);
    r.h.run();
    assert_eq!(focused(&r), Some(before));
}

#[test]
fn focus_returns_after_the_shortcut_sheet() {
    returns_to_card(|r| key(r, Key::F1));
}

#[test]
fn focus_returns_after_the_tag_editor() {
    returns_to_card(|r| key(r, Key::T));
}

#[test]
fn focus_returns_after_the_remove_dialog() {
    returns_to_card(|r| key(r, Key::Delete));
}

#[test]
fn focus_returns_after_the_context_menu() {
    returns_to_card(|r| key_mod(r, Modifiers::SHIFT, Key::F10));
}

#[test]
fn focus_returns_to_the_card_after_the_game_page() {
    let mut r = rig(Route::Library);
    r.h.run();
    open_game(&mut r, "Lantern Keep");
    key_mod(&mut r, Modifiers::ALT, Key::ArrowLeft);
    settle(&mut r);
    assert!(focused(&r).is_some_and(|f| f.starts_with("Lantern Keep. ")));
}
