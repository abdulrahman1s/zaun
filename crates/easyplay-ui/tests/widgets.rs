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
fn link_rows_are_buttons_that_report_the_row_and_list_their_points() {
    use easyplay_ui::theme::icons;
    use easyplay_ui::widgets::link_row::{LinkRow, Point, Points, Tone, link_rows};
    let mut h = Harness::builder()
        .with_size(Vec2::new(900.0, 400.0))
        .with_context_setup(setup)
        .build_ui_state(
            |ui, chosen: &mut Vec<usize>| {
                let points = [
                    Point {
                        icon: icons::WIFI_OFF,
                        text: "No internet or local network",
                        tone: Tone::Normal,
                    },
                    Point {
                        icon: icons::CAMERA,
                        text: "Can use your cameras",
                        tone: Tone::Warning,
                    },
                ];
                let rows = [
                    LinkRow::new(
                        icons::SHIELD_OFFLINE,
                        "Protection",
                        "Offline",
                        "Protection: Offline",
                    )
                    .points(Points::Listed {
                        name: "What this game can reach",
                        items: &points,
                        more: Some("and 3 more"),
                    }),
                    LinkRow::new(
                        icons::FOLDER,
                        "Game folder",
                        "Celeste",
                        "Game folder: Celeste",
                    )
                    .detail(Some("in Games"), Tone::Normal)
                    .external("Open in the file manager", Some("Ctrl+O")),
                ];
                if let Some(i) = link_rows(ui, Id::unique("rows"), &rows) {
                    chosen.push(i);
                }
            },
            Vec::new(),
        );
    h.run();
    h.get_by_role_and_label(Role::List, "What this game can reach");
    h.get_by_role_and_label(Role::ListItem, "Can use your cameras");
    h.get_by_role_and_label(Role::ListItem, "and 3 more");
    // The pointer on a point still presses the row beneath it.
    h.get_by_role_and_label(Role::ListItem, "No internet or local network")
        .click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Game folder: Celeste")
        .click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Protection: Offline")
        .focus();
    h.run();
    h.key_press(Key::Space);
    h.run();
    assert_eq!(*h.state(), vec![0, 1, 0]);
}

#[test]
fn permission_rows_flip_open_their_degrees_and_report_what_was_done() {
    use easyplay_ui::theme::icons;
    use easyplay_ui::widgets::permission_row::{
        Choice, Degree, Line, PermissionRow, Picked, Press, permission_rows,
    };
    let mut h = Harness::builder()
        .with_size(Vec2::new(900.0, 400.0))
        .with_context_setup(setup)
        .build_ui_state(
            |ui, done: &mut Vec<(usize, Picked)>| {
                let access = [
                    Choice::new("Can read"),
                    Choice {
                        label: "Can read and change",
                        unavailable: Some("The folder cannot be written to."),
                    },
                    Choice::new("Locate…"),
                ];
                let mut folder = PermissionRow::new(Id::unique("music"), icons::FOLDER, "Music");
                folder.lines = vec![Line::new("in Home")];
                folder.mark = Some("changed");
                folder.switch = Some(true);
                folder.delete = true;
                folder.degree = Some(Degree {
                    choices: &access,
                    chosen: 0,
                    value: "Can read".into(),
                    name: "Music: Can read".into(),
                });
                let kept = [
                    Choice::new("Private storage"),
                    Choice::new("A folder I choose…"),
                ];
                let mut save = PermissionRow::new(Id::unique("save"), icons::FLOPPY, "Save data");
                save.degree = Some(Degree {
                    choices: &kept,
                    chosen: 0,
                    value: "Private storage".into(),
                    name: "Save data: Private storage".into(),
                });
                let mut screen = PermissionRow::new(Id::unique("screen"), icons::MONITOR, "Screen");
                screen.lines = vec![Line::new("Shows its windows on your desktop")];
                let mut add =
                    PermissionRow::new(Id::unique("add"), icons::FOLDER_PLUS, "Add folder…");
                add.press = Press::Action;
                if let Some(picked) = permission_rows(ui, &[folder, save, screen, add]) {
                    done.push(picked);
                }
            },
            Vec::new(),
        );
    h.run();
    // A fact is not a control; a switch says whether it is on and what it means.
    h.get_by_role_and_label(Role::ListItem, "Screen");
    let folder = h.get_by_role_and_label(Role::Switch, "Music");
    assert_eq!(
        folder.accesskit_node().description().as_deref(),
        Some("in Home. changed")
    );
    // The degree beside a switch is a control of its own: pressing it does not flip the row.
    h.get_by_role_and_label(Role::ComboBox, "Music: Can read")
        .click();
    h.run();
    let no = h.get_by_role_and_label(Role::MenuItem, "Can read and change");
    assert!(no.accesskit_node().is_disabled());
    h.get_by_label("The folder cannot be written to.");
    h.get_by_role_and_label(Role::MenuItem, "Locate…").click();
    h.run();
    // Without a switch the whole row opens the degrees.
    h.get_by_role_and_label(Role::ComboBox, "Save data: Private storage")
        .click();
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "A folder I choose…")
        .click();
    h.run();
    h.get_by_role_and_label(Role::Switch, "Music").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Add folder…").click();
    h.run();
    h.get_by_role_and_label(Role::Switch, "Music").focus();
    h.run();
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(
        *h.state(),
        vec![
            (0, Picked::Degree(2)),
            (1, Picked::Degree(1)),
            (0, Picked::Switched(false)),
            (3, Picked::Pressed),
            (0, Picked::Delete)
        ]
    );
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
        .with_size(Vec2::new(1280.0, 2600.0))
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
