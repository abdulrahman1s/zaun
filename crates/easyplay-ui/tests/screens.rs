//! Statistics (UI-022, UF-19), settings (UI-023) and onboarding (UI-024, UF-1) tests and
//! snapshots.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Rig, now, rig_with, settle, settled_library};
use easyplay_core::config::schema::Theme;
use easyplay_ui::bridge::{FakeBackend, UiCommand, library_from};
use easyplay_ui::demo;
use easyplay_ui::route::{Route, SettingsTab};
use easyplay_ui::theme::ThemeChoice;
use egui::accesskit::Role;
use egui::{Key, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};

fn at(route: Route, size: Vec2, choice: ThemeChoice) -> Rig {
    let mut r = rig_with(settled_library(), route, size, choice);
    settle(&mut r);
    r
}

fn big() -> Vec2 {
    Vec2::new(1280.0, 900.0)
}

// ---- Statistics --------------------------------------------------------------------------

#[test]
fn statistics_show_tiles_chart_and_lists() {
    let r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.get_by_label_contains("Total playtime:");
    r.h.get_by_role_and_label(Role::Figure, "Playtime per day");
    r.h.get_all_by_label("Most played").next().unwrap();
    r.h.get_all_by_label("Recent sessions").next().unwrap();
}

#[test]
fn returning_to_statistics_shows_cached_results_in_the_first_frame() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.state_mut().state.nav.go_to(Route::Library);
    r.h.run();
    r.fake.clear_sent();
    r.h.state_mut().state.nav.go_to(Route::Statistics);
    r.h.step();
    r.h.get_by_label_contains("Total playtime:");
    assert!(!r.fake.sent().iter().any(|c| matches!(
        c,
        UiCommand::QueryStats {
            request: easyplay_core::stats::StatsRequest::LibraryOverview { .. },
            ..
        }
    )));
}

#[test]
fn chart_bars_move_with_arrow_keys_and_the_table_toggles() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.get_by_role_and_label(Role::Figure, "Playtime per day")
        .focus();
    r.h.run();
    r.h.key_press(Key::End);
    r.h.run();
    assert_eq!(r.h.state().state.stats.bar, Some(6));
    r.h.key_press(Key::ArrowLeft);
    r.h.run();
    assert_eq!(r.h.state().state.stats.bar, Some(5));
    r.h.get_by_label("Show as table").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Table, "Playtime per day");
}

#[test]
fn changing_the_range_queries_again() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.fake.clear_sent();
    r.h.get_by_label("30 days").click();
    settle(&mut r);
    assert_eq!(r.h.state().state.stats.range, 1);
    r.h.get_by_role_and_label(Role::Figure, "Playtime per day");
}

#[test]
fn statistics_empty_state() {
    let lib = library_from(Vec::new(), Vec::new(), 1, now());
    let mut r = rig_with(lib, Route::Statistics, big(), ThemeChoice::Dark);
    settle(&mut r);
    r.h.get_by_label("Nothing to show yet");
    r.h.snapshot("statistics_empty_dark_1280x900");
}

#[test]
fn snapshot_statistics_dark() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.run_steps(4);
    r.h.snapshot("statistics_dark_1280x900");
}

#[test]
fn snapshot_statistics_light() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Light);
    r.h.run_steps(4);
    r.h.snapshot("statistics_light_1280x900");
}

#[test]
fn snapshot_statistics_compact() {
    let mut r = at(
        Route::Statistics,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(4);
    r.h.snapshot("statistics_dark_640x480");
}

// ---- Settings ----------------------------------------------------------------------------

/// Every key of the preference schema has a control (UI-023 acceptance).
#[test]
fn every_preference_has_a_control() {
    let mut r = at(
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 3000.0),
        ThemeChoice::Dark,
    );
    for tab in easyplay_ui::screens::settings::TABS {
        r.h.state_mut().state.nav.replace(Route::Settings(tab));
        r.h.run_steps(3);
    }
    let rendered = easyplay_ui::screens::settings::rendered_keys(&r.h.ctx);
    let table =
        easyplay_core::config::load::to_table(&easyplay_core::config::Preferences::default())
            .unwrap();
    let mut missing = Vec::new();
    for (section, v) in table.iter() {
        let Some(t) = v.as_table() else { continue };
        for key in t.keys() {
            let full = format!("{section}.{key}");
            if !rendered.contains(&full) {
                missing.push(full);
            }
        }
    }
    assert!(missing.is_empty(), "no control for {missing:?}");
}

#[test]
fn a_toggle_applies_at_once_with_a_saved_tick() {
    let mut r = at(
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 1600.0),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Ask before removing a game").click();
    r.h.run();
    let sent = r.fake.sent();
    let preview = sent.iter().find_map(|c| match c {
        UiCommand::SetPreference { patch, preview } => Some((
            patch.keys().map(str::to_owned).collect::<Vec<_>>(),
            preview.clone(),
        )),
        _ => None,
    });
    let (keys, preview) = preview.expect("SetPreference sent");
    assert_eq!(keys, vec!["general.confirm_remove".to_owned()]);
    assert_eq!(
        preview.general.confirm_remove,
        !demo::preferences().general.confirm_remove
    );
    r.h.get_all_by_label("Saved").next().unwrap();
}

#[test]
fn theme_change_restyles_immediately() {
    let mut r = at(
        Route::Settings(SettingsTab::Appearance),
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Light").click();
    r.h.run();
    assert_eq!(r.h.ctx.theme(), egui::Theme::Light);
    let sent = r.fake.sent();
    assert!(sent.iter().any(|c| matches!(c, UiCommand::SetPreference { preview, .. } if preview.appearance.theme == Theme::Light)));
}

#[test]
fn settings_tabs_move_with_arrow_keys() {
    let mut r = at(
        Route::Settings(SettingsTab::General),
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_role_and_label(Role::Tab, "General").focus();
    r.h.run();
    r.h.key_press(Key::ArrowDown);
    r.h.run();
    assert_eq!(
        r.h.state().state.nav.current(),
        Route::Settings(SettingsTab::Appearance)
    );
}

#[test]
fn developer_mode_asks_first() {
    let mut r = at(
        Route::Settings(SettingsTab::Advanced),
        Vec2::new(1280.0, 1600.0),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Use Developer Mode").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Turn on Developer Mode?");
    r.h.key_press(Key::Enter);
    r.h.run();
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::SetPreference { .. }))
    );
}

fn settings_snap(tab: SettingsTab, name: &str, choice: ThemeChoice) {
    let mut r = at(Route::Settings(tab), Vec2::new(1280.0, 1000.0), choice);
    r.h.run_steps(4);
    r.h.snapshot(name);
}

#[test]
fn snapshot_settings_general_dark() {
    settings_snap(
        SettingsTab::General,
        "settings_general_dark_1280x1000",
        ThemeChoice::Dark,
    );
}

#[test]
fn snapshot_settings_appearance_light() {
    settings_snap(
        SettingsTab::Appearance,
        "settings_appearance_light_1280x1000",
        ThemeChoice::Light,
    );
}

#[test]
fn snapshot_settings_layers_dark() {
    settings_snap(
        SettingsTab::CompatibilityLayers,
        "settings_layers_dark_1280x1000",
        ThemeChoice::Dark,
    );
}

#[test]
fn snapshot_settings_compact() {
    let mut r = at(
        Route::Settings(SettingsTab::Protection),
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(4);
    r.h.snapshot("settings_protection_dark_640x480");
}

// ---- Onboarding --------------------------------------------------------------------------

fn onboarding(host: easyplay_ui::bridge::HostView, choice: ThemeChoice, size: Vec2) -> Rig {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.onboarding.completed = false;
    fake.publish_prefs(prefs);
    fake.publish_host(host);
    let mut r = common::rig_on(fake, Route::Library, size, choice);
    r.h.run();
    r
}

#[test]
fn uf1_all_checks_pass_and_onboarding_completes() {
    let mut r = onboarding(demo::host_view(), ThemeChoice::Dark, big());
    // Not completed in preferences: the window opens on onboarding.
    let fake = r.fake.clone();
    assert!(matches!(
        r.h.state().state.nav.current(),
        Route::Onboarding | Route::Library
    ));
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.run();
    r.h.get_by_label("Welcome to EasyPlay");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    r.h.get_by_label("Checking your system");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    r.h.get_by_label("Compatibility layer");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    r.h.get_by_label("Download covers from Steam's image server (shared.steamstatic.com)")
        .click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Start using EasyPlay")
        .click();
    r.h.run();
    let sent = fake.sent();
    assert!(sent.contains(&UiCommand::CompleteOnboarding { skipped: false }));
    assert!(sent.iter().any(|c| matches!(c, UiCommand::SetPreference { patch, .. } if patch.keys().any(|k| k == "artwork.online"))));
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn a_blocking_failure_cannot_be_passed_and_skip_leaves_a_banner() {
    let mut r = onboarding(
        demo::host_view_without_namespaces(),
        ThemeChoice::Dark,
        big(),
    );
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.state_mut().state.onboarding.step = easyplay_ui::vm::screens::OnboardingStep::SystemCheck;
    r.h.run();
    r.h.get_by_label_contains(
        "Private namespaces: turned off on this system, failed; fix available",
    );
    let cont = r.h.get_by_role_and_label(Role::Button, "Continue");
    assert!(cont.accesskit_node().is_disabled());
    r.h.get_by_label("Fix the failed check to continue");
    r.h.get_by_role_and_label(Role::Button, "Skip for now")
        .click();
    r.h.run();
    assert!(
        r.h.state()
            .state
            .banners
            .top()
            .is_some_and(|b| b.key == "host")
    );
}

#[test]
fn snapshot_onboarding_check_failure_dark() {
    let mut r = onboarding(
        demo::host_view_without_namespaces(),
        ThemeChoice::Dark,
        big(),
    );
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.state_mut().state.onboarding.step = easyplay_ui::vm::screens::OnboardingStep::SystemCheck;
    r.h.run_steps(4);
    r.h.snapshot("onboarding_check_dark_1280x900");
}

#[test]
fn snapshot_onboarding_welcome_light() {
    let mut r = onboarding(demo::host_view(), ThemeChoice::Light, big());
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.run_steps(4);
    r.h.snapshot("onboarding_welcome_light_1280x900");
}

#[test]
fn snapshot_onboarding_handler_compact() {
    let mut r = onboarding(
        demo::host_view(),
        ThemeChoice::Dark,
        Vec2::new(640.0, 480.0),
    );
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.state_mut().state.onboarding.step = easyplay_ui::vm::screens::OnboardingStep::Handler;
    r.h.run_steps(4);
    r.h.snapshot("onboarding_handler_dark_640x480");
}
