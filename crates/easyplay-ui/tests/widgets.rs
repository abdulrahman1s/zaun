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
use egui_kittest::kittest::Queryable;

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
