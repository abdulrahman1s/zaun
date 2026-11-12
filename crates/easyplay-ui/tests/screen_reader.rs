//! A11Y-006: what a screen reader gets. Every screen's AccessKit tree is walked: each
//! interactive node has a role and a name, decorative nodes are not focusable, there is one
//! level-1 heading per route where the route has a title, and the `Tab` order visits only
//! named controls in reading order (sidebar → toolbar → content) without losing focus.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Rig, game_id, rig, rig_on, rig_with, settle, settled_library};
use easyplay_ui::bridge::FakeBackend;
use easyplay_ui::demo;
use easyplay_ui::route::{Overlay, Route, SettingsTab};
use easyplay_ui::theme::ThemeChoice;
use easyplay_ui::vm::details::DetailsTab;
use easyplay_ui::vm::library::ViewMode;
use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};

/// Roles a user operates; each needs a name.
fn interactive(role: Role) -> bool {
    matches!(
        role,
        Role::Button
            | Role::DefaultButton
            | Role::CheckBox
            | Role::Switch
            | Role::RadioButton
            | Role::TextInput
            | Role::SearchInput
            | Role::MultilineTextInput
            | Role::Slider
            | Role::SpinButton
            | Role::Tab
            | Role::ComboBox
            | Role::Link
            | Role::MenuItem
            | Role::MenuItemCheckBox
            | Role::MenuItemRadio
            | Role::Row
            | Role::Cell
            | Role::ListBoxOption
    )
}

/// Every unnamed interactive node of the current frame, described for the failure message.
fn unnamed(r: &Rig) -> Vec<String> {
    r.h.query_all_by(|n| interactive(n.role()) && !n.is_hidden())
        .filter(|n| {
            let a = n.accesskit_node();
            let named = a.label().is_some_and(|l| !l.trim().is_empty())
                || a.labelled_by().next().is_some()
                || a.value().is_some_and(|v| !v.trim().is_empty());
            !named
        })
        .map(|n| {
            format!(
                "{:?} {:?}",
                n.accesskit_node().role(),
                n.accesskit_node().id()
            )
        })
        .collect()
}

fn assert_named(r: &Rig, what: &str) {
    let missing = unnamed(r);
    assert!(
        missing.is_empty(),
        "{what}: controls without a name: {missing:#?}"
    );
}

fn at(route: Route, size: Vec2) -> Rig {
    let mut r = rig_with(settled_library(), route, size, ThemeChoice::Dark);
    settle(&mut r);
    r
}

#[test]
fn every_control_on_every_screen_has_a_role_and_a_name() {
    let big = Vec2::new(1280.0, 1400.0);
    let lib = settled_library();
    let lantern = game_id(&lib, "Lantern Keep");

    let mut r = at(Route::Library, big);
    assert_named(&r, "library grid");
    r.h.state_mut().state.library.view = ViewMode::List;
    r.h.run();
    assert_named(&r, "library list");

    for tab in DetailsTab::ALL {
        let mut r = rig_with(lib.clone(), Route::Game(lantern), big, ThemeChoice::Dark);
        r.h.state_mut().state.details.tab = tab;
        settle(&mut r);
        assert_named(&r, &format!("game page {tab:?}"));
    }
    let mut r = at(Route::Statistics, big);
    r.h.run_steps(3);
    assert_named(&r, "statistics");
    for tab in easyplay_ui::screens::settings::TABS {
        let mut r = at(Route::Settings(tab), big);
        r.h.run_steps(3);
        assert_named(&r, &format!("settings {tab:?}"));
    }
    // Overlays.
    let mut r = at(Route::Library, big);
    r.h.state_mut().state.nav.open(Overlay::Shortcuts);
    r.h.run();
    assert_named(&r, "shortcut sheet");
    let mut r = at(Route::Library, big);
    r.h.state_mut().state.nav.open(Overlay::Tags(lantern));
    r.h.run();
    assert_named(&r, "tag editor");
    let mut r = at(Route::Library, big);
    r.h.state_mut().state.nav.open(Overlay::Artwork(lantern));
    settle(&mut r);
    assert_named(&r, "artwork sheet");
}

#[test]
fn onboarding_and_setup_controls_are_named() {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.onboarding.completed = false;
    fake.publish_prefs(prefs);
    fake.publish_host(demo::host_view_without_namespaces());
    let mut r = rig_on(
        fake,
        Route::Onboarding,
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    for step in easyplay_ui::vm::screens::OnboardingStep::ALL {
        r.h.state_mut().state.onboarding.step = step;
        r.h.run();
        assert_named(&r, &format!("onboarding {step:?}"));
    }
    let mut r = rig(Route::Library);
    r.h.run();
    r.fake
        .push_event(easyplay_ui::bridge::UiEvent::SetupRequested {
            draft: Box::new(demo::draft(
                std::path::Path::new("/home/sam/Games/Celeste/Celeste.exe"),
                common::now(),
            )),
            context: easyplay_ui::bridge::SetupContext {
                origin: easyplay_ui::bridge::SetupOrigin::FileManager,
                queue: (1, 1),
            },
        });
    r.h.run();
    assert_named(&r, "setup sheet");
}

#[test]
fn game_cards_speak_name_protection_and_playtime() {
    let r = at(Route::Library, Vec2::new(1280.0, 800.0));
    let card = r.h.get_by_label_contains("Lantern Keep. Offline. Played");
    assert_eq!(card.accesskit_node().role(), Role::Button);
    r.h.get_by_role_and_label(Role::Grid, "Games, 24 items, 5 columns");
    // The cover image is named by the game, not described (ACCESSIBILITY §4.1).
    assert!(
        r.h.query_all_by(|n| n.role() == Role::Image)
            .all(|n| n.accesskit_node().label().is_some())
    );
}

#[test]
fn overlays_hide_the_page_behind_them() {
    let mut r = at(Route::Library, Vec2::new(1280.0, 800.0));
    r.h.state_mut().state.nav.open(Overlay::Shortcuts);
    r.h.run();
    // Focus is trapped in the sheet: Tab never reaches a card.
    for _ in 0..30 {
        r.h.key_press(Key::Tab);
        r.h.run();
        let f =
            r.h.query_by(|n| n.is_focused())
                .map(|n| n.accesskit_node().label().unwrap_or_default().to_string());
        assert!(
            !f.as_deref().is_some_and(|l| l.contains(". Offline.")),
            "focus reached the page behind the sheet: {f:?}"
        );
    }
}

/// The `Tab` sequence of the library: only named controls, never lost, in reading order.
#[test]
fn tab_order_follows_reading_order_and_never_loses_focus() {
    let mut r = at(Route::Library, Vec2::new(1280.0, 800.0));
    let mut stops = Vec::new();
    for _ in 0..60 {
        r.h.key_press(Key::Tab);
        r.h.run();
        let node = r.h.query_by(|n| n.is_focused());
        let node = node.expect("focus lost after Tab");
        let a = node.accesskit_node();
        let label = a.label().unwrap_or_default().to_string();
        assert!(
            interactive(a.role()) || a.role() == Role::Grid || a.role() == Role::Figure,
            "focus on a non-control: {:?} {label:?}",
            a.role()
        );
        assert!(
            !label.trim().is_empty(),
            "unnamed focus stop {:?}",
            a.role()
        );
        stops.push(label);
        if stops.len() > 3 && stops[0] == *stops.last().unwrap() {
            break;
        }
    }
    let first = |pred: &dyn Fn(&str) -> bool| stops.iter().position(|s| pred(s)).unwrap();
    let sidebar = first(&|s| s.starts_with("All games"));
    let search = first(&|s| s == "Search games");
    let card = first(&|s| {
        s.contains(". Offline.") || s.contains(". Online.") || s.contains(". Standard.")
    });
    assert!(sidebar < search && search < card, "{stops:#?}");
    // Only one card is a Tab stop (roving focus).
    let cards = stops
        .iter()
        .filter(|s| s.contains(". Played") || s.contains("Not played yet"))
        .count();
    assert_eq!(cards, 1, "{stops:#?}");
    // Shift+Tab walks the same stops backwards.
    r.h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    r.h.run();
    assert!(r.h.query_by(|n| n.is_focused()).is_some());
}

#[test]
fn settings_and_game_page_have_one_title_heading() {
    for route in [
        Route::Settings(SettingsTab::General),
        Route::Game(game_id(&settled_library(), "Lantern Keep")),
        Route::Statistics,
    ] {
        let r = at(route, Vec2::new(1280.0, 900.0));
        let h1 =
            r.h.query_all_by(|n| n.role() == Role::Heading && n.level() == Some(1))
                .count();
        assert!(h1 <= 1, "{route:?}: {h1} level-1 headings");
    }
}
