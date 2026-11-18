//! Review renders of Statistics, Settings, the welcome steps and the damaged library
//! (throwaway): `cargo test -p zaun-ui
//! --features review --test review review_pages:: -- --ignored
//! --test-threads=1`. Images land in `target/review/pages-*.png`, beside the prototype's
//! reference screenshots of the same names.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use std::path::PathBuf;

use common::*;
use egui::Vec2;
use egui_kittest::kittest::Queryable;
use zaun_core::clock::Timestamp;
use zaun_core::error::{ErrorCode, catalog};
use zaun_core::service::{Diagnostics, RecoveryReason, RecoveryState};
use zaun_ui::bridge::FakeBackend;
use zaun_ui::demo;
use zaun_ui::route::{Route, SettingsTab};
use zaun_ui::theme::ThemeChoice;
use zaun_ui::vm::screens::OnboardingStep;

fn save(r: &mut Rig, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    let image = r.h.render().expect("render");
    image.save(dir.join(format!("pages-{name}.png"))).unwrap();
}

fn size() -> Vec2 {
    Vec2::new(1280.0, 800.0)
}

fn settled(r: &mut Rig) {
    settle(r);
    r.h.run_steps(40);
    serve(r);
    r.h.run_steps(40);
}

fn scroll_down(r: &mut Rig) {
    r.h.hover_at(egui::pos2(400.0, 400.0));
    r.h.run_steps(2);
    for _ in 0..10 {
        r.h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -400.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
        r.h.run_steps(2);
    }
    r.h.hover_at(egui::pos2(1270.0, 790.0));
    r.h.run_steps(40);
}

fn stats(range: usize, table: bool, w: f32, choice: ThemeChoice, name: &str) {
    let mut r = rig_with(
        settled_library(),
        Route::Statistics,
        Vec2::new(w, 800.0),
        choice,
    );
    r.h.state_mut().state.stats.range = range;
    r.h.state_mut().state.stats.table = table;
    settled(&mut r);
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn stats_7() {
    stats(0, false, 1280.0, ThemeChoice::Dark, "stats-7");
}

#[test]
#[ignore = "review render"]
fn stats_30() {
    stats(1, false, 1280.0, ThemeChoice::Dark, "stats-30");
}

#[test]
#[ignore = "review render"]
fn stats_365() {
    stats(2, false, 1280.0, ThemeChoice::Dark, "stats-365");
}

#[test]
#[ignore = "review render"]
fn stats_all() {
    stats(3, false, 1280.0, ThemeChoice::Dark, "stats-all");
}

#[test]
#[ignore = "review render"]
fn stats_7_table() {
    stats(0, true, 1280.0, ThemeChoice::Dark, "stats-7-table");
}

#[test]
#[ignore = "review render"]
fn stats_7_light() {
    stats(0, false, 1280.0, ThemeChoice::Light, "stats-7-light");
}

#[test]
#[ignore = "review render"]
fn stats_7_900() {
    stats(0, false, 900.0, ThemeChoice::Dark, "stats-7-900");
}

fn settings(tab: SettingsTab, choice: ThemeChoice, scrolled: bool, name: &str) {
    let mut r = rig_with(settled_library(), Route::Settings(tab), size(), choice);
    settled(&mut r);
    if scrolled {
        scroll_down(&mut r);
    }
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn settings_general() {
    settings(
        SettingsTab::General,
        ThemeChoice::Dark,
        false,
        "settings-general",
    );
}

#[test]
#[ignore = "review render"]
fn settings_general_scrolled() {
    settings(
        SettingsTab::General,
        ThemeChoice::Dark,
        true,
        "settings-general-scrolled",
    );
}

#[test]
#[ignore = "review render"]
fn settings_appearance() {
    settings(
        SettingsTab::Appearance,
        ThemeChoice::Dark,
        false,
        "settings-appearance",
    );
}

#[test]
#[ignore = "review render"]
fn settings_appearance_light() {
    settings(
        SettingsTab::Appearance,
        ThemeChoice::Light,
        false,
        "settings-appearance-light",
    );
}

#[test]
#[ignore = "review render"]
fn settings_appearance_hc() {
    settings(
        SettingsTab::Appearance,
        ThemeChoice::HighContrast,
        false,
        "settings-appearance-hc",
    );
}

#[test]
#[ignore = "review render"]
fn settings_protection() {
    settings(
        SettingsTab::Protection,
        ThemeChoice::Dark,
        false,
        "settings-protection",
    );
}

#[test]
#[ignore = "review render"]
fn settings_layers() {
    settings(
        SettingsTab::CompatibilityLayers,
        ThemeChoice::Dark,
        false,
        "settings-layers",
    );
}

#[test]
#[ignore = "review render"]
fn settings_integration() {
    settings(
        SettingsTab::Integration,
        ThemeChoice::Dark,
        false,
        "settings-integration",
    );
}

#[test]
#[ignore = "review render"]
fn settings_advanced() {
    settings(
        SettingsTab::Advanced,
        ThemeChoice::Dark,
        false,
        "settings-advanced",
    );
}

#[test]
#[ignore = "review render"]
fn settings_advanced_scrolled() {
    settings(
        SettingsTab::Advanced,
        ThemeChoice::Dark,
        true,
        "settings-advanced-scrolled",
    );
}

#[test]
#[ignore = "review render"]
fn settings_about() {
    settings(
        SettingsTab::About,
        ThemeChoice::Dark,
        false,
        "settings-about",
    );
}

fn welcome(host: zaun_ui::bridge::HostView, step: OnboardingStep, name: &str) {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.onboarding.completed = false;
    fake.publish_prefs(prefs);
    fake.publish_host(host);
    let mut r = rig_on(fake, Route::Onboarding, size(), ThemeChoice::Dark);
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.state_mut().state.onboarding.step = step;
    r.h.run_steps(40);
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn welcome_1() {
    welcome(demo::host_view(), OnboardingStep::Welcome, "welcome-1");
}

#[test]
#[ignore = "review render"]
fn welcome_2_fail() {
    welcome(
        demo::host_view_without_namespaces(),
        OnboardingStep::SystemCheck,
        "welcome-2-fail",
    );
}

#[test]
#[ignore = "review render"]
fn welcome_2_ok() {
    welcome(
        demo::host_view(),
        OnboardingStep::SystemCheck,
        "welcome-2-ok",
    );
}

#[test]
#[ignore = "review render"]
fn welcome_3() {
    welcome(demo::host_view(), OnboardingStep::Handler, "welcome-3");
}

#[test]
#[ignore = "review render"]
fn welcome_layer() {
    welcome(demo::host_view(), OnboardingStep::Layer, "welcome-layer");
}

#[test]
#[ignore = "review render"]
fn lib_damaged() {
    let mut r = rig(Route::Library);
    let backup = PathBuf::from("/home/sam/.local/share/zaun/backups/daily-20261115.db");
    let err = catalog::describe(
        &zaun_core::error::Error::new(ErrorCode::DbCorrupt),
        &catalog::DescribeCtx::plain(),
    );
    r.fake.publish_diagnostics(Diagnostics {
        recovery: Some(RecoveryState {
            reason: RecoveryReason::Damaged {
                backups: vec![(
                    backup,
                    Timestamp::from_millis(now().as_millis() - 86_400_000),
                    12,
                )],
                repaired_games: Some(9),
                missing_games: vec!["Celeste".into(), "Hades".into()],
            },
            error: err,
        }),
        ..Diagnostics::default()
    });
    settled(&mut r);
    save(&mut r, "lib-damaged");
}

fn stats_then(name: &str, act: impl FnOnce(&mut Rig)) {
    let mut r = rig_with(
        settled_library(),
        Route::Statistics,
        size(),
        ThemeChoice::Dark,
    );
    settled(&mut r);
    act(&mut r);
    settled(&mut r);
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn stats_7_show_all() {
    stats_then("stats-7-show-all", |r| {
        r.h.get_by_label("Show all 9").click();
    });
}

#[test]
#[ignore = "review render"]
fn stats_history() {
    stats_then("stats-history", |r| {
        r.h.get_by_label("Show play history").click();
    });
}

#[test]
#[ignore = "review render"]
fn stats_7_hover() {
    stats_then("stats-7-hover", |r| {
        r.h.hover_at(egui::pos2(210.0, 300.0));
        r.h.run_steps(60);
    });
}
