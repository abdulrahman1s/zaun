//! Widget tests (UI-004 … UI-006): activation by mouse and keyboard, accessible names,
//! disabled explanation, focus return from overlays, and the gallery snapshots.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use easyplay_testkit::FakeClock;
use easyplay_ui::bridge::{FakeBackend, UiBridge};
use easyplay_ui::route::Route;
use easyplay_ui::theme::{self, Appearance, ThemeChoice};
use easyplay_ui::widgets::button::Button;
use easyplay_ui::widgets::modal::{self, Action, DialogOutcome};
use easyplay_ui::widgets::segmented::{Segment, segmented};
use easyplay_ui::widgets::toggle::toggle;
use easyplay_ui::{EasyPlayApp, demo};
use egui::accesskit::Role;
use egui::{Id, Key, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

fn setup(ctx: &egui::Context) {
    theme::install(
        ctx,
        &Appearance {
            theme: ThemeChoice::Dark,
            ..Appearance::default()
        },
    );
}

#[derive(Default)]
struct Clicks {
    primary: u32,
    disabled: u32,
}

#[test]
fn button_activates_by_mouse_and_keyboard_and_explains_when_disabled() {
    let mut h = Harness::builder().with_context_setup(setup).build_ui_state(
        |ui, s: &mut Clicks| {
            if ui.add(Button::primary("Save")).clicked() {
                s.primary += 1;
            }
            if ui
                .add(Button::new("Remove").disabled(Some("Nothing selected.")))
                .clicked()
            {
                s.disabled += 1;
            }
        },
        Clicks::default(),
    );
    h.get_by_role_and_label(Role::Button, "Save").click();
    h.run();
    h.get_by_label("Save").focus();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().primary, 2);
    h.get_by_label("Remove").click();
    h.run();
    assert_eq!(h.state().disabled, 0);
}

#[test]
fn doors_are_whole_row_buttons_that_say_where_they_lead() {
    use easyplay_ui::widgets::rows::{Leads, door, fact_text};
    let mut h = Harness::builder()
        .with_size(Vec2::new(900.0, 400.0))
        .with_context_setup(setup)
        .build_ui_state(
            |ui, opened: &mut Vec<usize>| {
                fact_text(ui, "Last played", "Yesterday");
                let doors = [
                    (
                        "Protection",
                        "Protection: Offline. Open the Protection tab.",
                        Leads::Tab,
                    ),
                    (
                        "Game folder",
                        "Game folder: Celeste. Open in the file manager.",
                        Leads::Outside,
                    ),
                ];
                for (i, (label, name, leads)) in doors.into_iter().enumerate() {
                    let r = door(ui, label, name, leads, |ui| {
                        ui.label("value");
                    });
                    if r.clicked() {
                        opened.push(i);
                    }
                }
            },
            Vec::new(),
        );
    h.run();
    // A fact is not a control.
    assert!(
        h.query_by_role_and_label(Role::Button, "Last played")
            .is_none()
    );
    h.get_by_role_and_label(
        Role::Button,
        "Game folder: Celeste. Open in the file manager.",
    )
    .click();
    h.run();
    h.get_by_role_and_label(
        Role::Button,
        "Protection: Offline. Open the Protection tab.",
    )
    .focus();
    h.run();
    h.key_press(Key::Space);
    h.run();
    assert_eq!(*h.state(), vec![1, 0]);
}

#[derive(Default)]
struct Permission {
    on: bool,
    degree: usize,
}

#[test]
fn a_permission_switch_flips_and_its_degree_is_picked_under_its_label() {
    use easyplay_ui::widgets::rows::SettingRow;
    use easyplay_ui::widgets::select::select_inline;
    use easyplay_ui::widgets::toggle::switch;
    let mut h = Harness::builder()
        .with_size(Vec2::new(900.0, 400.0))
        .with_context_setup(setup)
        .build_ui_state(
            |ui, p: &mut Permission| {
                let Permission { on, degree } = p;
                SettingRow::new("Game controllers").show_with(
                    ui,
                    |ui| {
                        select_inline(
                            ui,
                            "Which controllers",
                            degree,
                            &["Connected when the game starts", "All input devices"],
                        );
                    },
                    |ui| switch(ui, on, "Game controllers"),
                );
            },
            Permission::default(),
        );
    h.run();
    h.get_by_role_and_label(Role::Switch, "Game controllers")
        .click();
    h.run();
    assert!(h.state().on);
    h.get_by_role_and_label(Role::Switch, "Game controllers")
        .focus();
    h.run();
    h.key_press(Key::Space);
    h.run();
    assert!(!h.state().on);
    // The degree is a control of its own: picking one does not flip the switch.
    h.get_by_role_and_label(
        Role::ComboBox,
        "Which controllers: Connected when the game starts",
    )
    .click();
    h.run();
    h.get_by_role_and_label(Role::MenuItemRadio, "All input devices")
        .click();
    h.run();
    assert_eq!(h.state().degree, 1);
    assert!(!h.state().on);
}

#[test]
fn a_scope_word_names_its_count_and_a_filter_word_its_remove_button() {
    use easyplay_ui::widgets::word::{Word, word_on};
    let mut h = Harness::builder().with_context_setup(setup).build_ui_state(
        |ui, removed: &mut bool| {
            ui.horizontal(|ui| {
                let _ = ui.add(Word::new("Favourites").count(3).pressed(true));
                if word_on(ui, "Online", None, "Remove Online").clicked() {
                    *removed = true;
                }
            });
        },
        false,
    );
    h.run();
    let scope = h.get_by_role_and_label(Role::Switch, "Favourites, 3");
    assert_eq!(
        scope.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    h.get_by_role_and_label(Role::Button, "Remove Online")
        .click();
    h.run();
    assert!(*h.state());
}

#[test]
fn toggle_row_flips_on_click_and_space() {
    let mut h = Harness::builder().with_context_setup(setup).build_ui_state(
        |ui, on: &mut bool| {
            toggle(ui, on, "Show playtime", Some("Under each title."));
        },
        false,
    );
    h.get_by_role_and_label(Role::Switch, "Show playtime")
        .click();
    h.run();
    assert!(*h.state());
    h.get_by_label("Show playtime").focus();
    h.run();
    h.key_press(Key::Space);
    h.run();
    assert!(!*h.state());
}

#[test]
fn segmented_moves_with_arrow_keys() {
    let mut h = Harness::builder().with_context_setup(setup).build_ui_state(
        |ui, sel: &mut usize| {
            let opts = [
                Segment {
                    icon: None,
                    label: "Offline",
                },
                Segment {
                    icon: None,
                    label: "Online",
                },
                Segment {
                    icon: None,
                    label: "Standard",
                },
            ];
            segmented(ui, "Protection", sel, &opts, false);
        },
        0usize,
    );
    h.get_by_role_and_label(Role::RadioButton, "Offline")
        .focus();
    h.run();
    h.key_press(Key::ArrowRight);
    h.run();
    h.key_press(Key::ArrowRight);
    h.run();
    assert_eq!(*h.state(), 2);
    assert!(h.get_by_label("Standard").is_focused());
}

#[derive(Default)]
struct DialogState {
    open: bool,
    outcome: Option<DialogOutcome>,
}

#[test]
fn dialog_cancels_on_escape_and_returns_focus_to_the_invoker() {
    let mut h = Harness::builder().with_context_setup(setup).build_ui_state(
        |ui, s: &mut DialogState| {
            modal::apply_focus_return(ui.ctx());
            if ui.add(Button::new("Remove game…")).clicked() {
                s.open = true;
            }
            if s.open {
                let o = modal::dialog(
                    ui.ctx(),
                    Id::unique("test.dialog"),
                    None,
                    "Remove Lantern Keep?",
                    "This removes the game from your library.",
                    &[
                        Action {
                            label: "Keep game",
                            variant: easyplay_ui::widgets::button::Variant::Secondary,
                        },
                        Action {
                            label: "Remove game",
                            variant: easyplay_ui::widgets::button::Variant::Danger,
                        },
                    ],
                    0,
                );
                if o != DialogOutcome::Open {
                    s.open = false;
                    s.outcome = Some(o);
                }
            }
        },
        DialogState::default(),
    );
    h.get_by_label("Remove game…").focus();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().open);
    // The safe default has focus.
    assert!(h.get_by_label("Keep game").is_focused());
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().outcome, Some(DialogOutcome::Cancelled));
    assert!(h.get_by_label("Remove game…").is_focused());
}

fn gallery(choice: ThemeChoice) -> Harness<'static, EasyPlayApp> {
    let now = demo::baseline_now();
    let appearance = Appearance {
        theme: choice,
        ..Appearance::default()
    };
    let app = EasyPlayApp::new(
        UiBridge::new(Box::new(FakeBackend::new(demo::library(now)))),
        Arc::new(FakeClock::new(now)),
        appearance,
        Route::Gallery,
        0,
    );
    Harness::builder()
        .with_size(Vec2::new(1280.0, 6000.0))
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_| app)
}

#[test]
fn snapshot_gallery_dark() {
    let mut h = gallery(ThemeChoice::Dark);
    h.run_steps(6);
    h.snapshot("gallery_dark");
}

#[test]
fn snapshot_gallery_light() {
    let mut h = gallery(ThemeChoice::Light);
    h.run_steps(6);
    h.snapshot("gallery_light");
}

#[test]
fn gallery_theme_switch_restyles_live() {
    let mut h = gallery(ThemeChoice::Dark);
    h.run_steps(3);
    h.get_all_by_role_and_label(Role::RadioButton, "Light")
        .next()
        .unwrap()
        .click();
    h.run_steps(3);
    assert_eq!(theme::theme(&h.ctx).palette.kind, theme::ThemeKind::Light);
}
