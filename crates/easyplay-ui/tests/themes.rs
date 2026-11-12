//! A11Y-004: the high-contrast theme on the key screens, and following the desktop's
//! appearance preferences (high contrast, reduced motion, text scale).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{game_id, now, rig, rig_with, settle, settled_library};
use easyplay_ui::bridge::{SetupContext, SetupOrigin, UiEvent};
use easyplay_ui::demo;
use easyplay_ui::route::{Route, SettingsTab};
use easyplay_ui::theme::{self, SystemPrefs, SystemScheme, ThemeChoice, ThemeKind};
use easyplay_ui::vm::details::DetailsTab;
use egui::Vec2;

#[test]
fn snapshots_in_high_contrast() {
    let mut results = egui_kittest::SnapshotResults::new();
    let lib = settled_library();
    let lantern = game_id(&lib, "Lantern Keep");
    let size = Vec2::new(1280.0, 900.0);

    let mut r = rig_with(
        lib.clone(),
        Route::Game(lantern),
        size,
        ThemeChoice::HighContrast,
    );
    r.h.state_mut().state.details.tab = DetailsTab::Protection;
    settle(&mut r);
    r.h.run_steps(4);
    r.h.snapshot("game_protection_hc_1280x900");
    results.extend_harness(&mut r.h);

    let mut r = rig_with(
        lib.clone(),
        Route::Settings(SettingsTab::Appearance),
        size,
        ThemeChoice::HighContrast,
    );
    settle(&mut r);
    r.h.run_steps(4);
    r.h.snapshot("settings_appearance_hc_1280x900");
    results.extend_harness(&mut r.h);

    let mut r = rig_with(
        lib.clone(),
        Route::Statistics,
        size,
        ThemeChoice::HighContrast,
    );
    settle(&mut r);
    r.h.run_steps(4);
    r.h.snapshot("statistics_hc_1280x900");
    results.extend_harness(&mut r.h);

    let mut r = rig_with(lib, Route::Library, size, ThemeChoice::HighContrast);
    r.h.run();
    r.fake.push_event(UiEvent::SetupRequested {
        draft: Box::new(demo::draft(
            std::path::Path::new("/home/sam/Games/Celeste/Celeste.exe"),
            now(),
        )),
        context: SetupContext {
            origin: SetupOrigin::FileManager,
            queue: (1, 1),
        },
    });
    r.h.run_steps(8);
    r.h.snapshot("setup_hc_1280x900");
    results.extend_harness(&mut r.h);
}

fn system(high_contrast: bool, reduced_motion: Option<bool>, text_scale: f32) -> SystemPrefs {
    SystemPrefs {
        scheme: Some(SystemScheme::Light),
        high_contrast,
        reduced_motion,
        text_scale,
    }
}

/// The demo preferences say "system" for theme and reduced motion.
fn follows_system(prefs: &easyplay_core::config::Preferences) -> bool {
    use easyplay_core::config::schema::{ReduceMotion, Theme};
    prefs.appearance.theme == Theme::System
        && prefs.appearance.reduce_motion == ReduceMotion::System
}

#[test]
fn the_desktop_high_contrast_and_reduced_motion_are_followed_live() {
    let mut r = rig(Route::Library);
    let mut prefs = demo::preferences();
    prefs.appearance.theme = easyplay_core::config::schema::Theme::System;
    prefs.appearance.reduce_motion = easyplay_core::config::schema::ReduceMotion::System;
    assert!(follows_system(&prefs));
    r.fake.publish_prefs(prefs);
    r.h.run();
    let ctx = r.h.ctx.clone();
    r.h.state_mut()
        .set_system_prefs(&ctx, system(true, Some(true), 1.0));
    r.h.run();
    assert_eq!(theme::theme(&ctx).palette.kind, ThemeKind::HighContrast);
    assert!(theme::theme(&ctx).motion.reduced);
    assert_eq!(ctx.global_style().animation_time, 0.0);
    // The desktop turns them off again.
    r.h.state_mut()
        .set_system_prefs(&ctx, system(false, Some(false), 1.0));
    r.h.run();
    assert_ne!(theme::theme(&ctx).palette.kind, ThemeKind::HighContrast);
    assert!(!theme::theme(&ctx).motion.reduced);
}

#[test]
fn an_explicit_preference_wins_over_the_desktop() {
    let mut r = rig(Route::Library);
    let mut prefs = demo::preferences();
    prefs.appearance.theme = easyplay_core::config::schema::Theme::Dark;
    prefs.appearance.reduce_motion = easyplay_core::config::schema::ReduceMotion::Off;
    r.fake.publish_prefs(prefs);
    r.h.run();
    let ctx = r.h.ctx.clone();
    r.h.state_mut()
        .set_system_prefs(&ctx, system(true, Some(true), 1.0));
    r.h.run();
    assert_eq!(theme::theme(&ctx).palette.kind, ThemeKind::Dark);
    assert!(!theme::theme(&ctx).motion.reduced);
}

#[test]
fn the_desktop_text_scale_multiplies_the_ui_scale() {
    let mut r = rig(Route::Library);
    r.h.run();
    let ctx = r.h.ctx.clone();
    r.h.state_mut()
        .set_system_prefs(&ctx, system(false, None, 1.25));
    r.h.run();
    let ui_scale = demo::preferences().appearance.ui_scale as f32;
    assert!((ctx.zoom_factor() - 1.25 * ui_scale).abs() < 1e-3);
}
