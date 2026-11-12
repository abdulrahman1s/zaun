//! PERF-003: an idle window paints nothing; a running game repaints at 1 Hz
//! (PERFORMANCE §1 "Idle CPU", §8).
//!
//! The test plays the role of eframe's event loop: after the UI settled, it reads the repaint
//! delay the last pass asked for, advances the clocks by that much, runs one frame, and
//! counts frames over two simulated seconds.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use common::{Rig, game_id, rebuild, rig_with, settle, settled_library};
use easyplay_core::model::Activity;
use easyplay_ui::bridge::LibraryView;
use easyplay_ui::route::{Route, SettingsTab};
use easyplay_ui::theme::ThemeChoice;
use easyplay_ui::vm::details::DetailsTab;
use easyplay_ui::vm::library::ViewMode;
use egui::{Vec2, ViewportId};

const WINDOW: Duration = Duration::from_secs(2);

fn repaint_delay(r: &Rig) -> Duration {
    r.h.output()
        .viewport_output
        .get(&ViewportId::ROOT)
        .map_or(Duration::MAX, |v| v.repaint_delay)
}

/// Frames a real event loop would paint over [`WINDOW`], and the shortest delay asked for.
fn frames_over_window(r: &mut Rig) -> (u32, Duration) {
    let mut t = Duration::ZERO;
    let mut frames = 0;
    let mut shortest = Duration::MAX;
    loop {
        let d = repaint_delay(r);
        shortest = shortest.min(d);
        if d == Duration::MAX || t.saturating_add(d) > WINDOW {
            return (frames, shortest);
        }
        let d = d.max(Duration::from_millis(1));
        t += d;
        r.clock.advance(d);
        r.h.step();
        frames += 1;
        assert!(frames < 1_000, "busy repaint loop");
    }
}

fn routes(lib: &LibraryView) -> Vec<(String, Route, Option<DetailsTab>, Option<ViewMode>)> {
    let g = game_id(lib, &lib.games[0].name);
    let mut v = vec![
        (
            "library grid".into(),
            Route::Library,
            None,
            Some(ViewMode::Grid),
        ),
        (
            "library list".into(),
            Route::Library,
            None,
            Some(ViewMode::List),
        ),
        ("statistics".into(), Route::Statistics, None, None),
        ("onboarding".into(), Route::Onboarding, None, None),
    ];
    for tab in DetailsTab::ALL {
        v.push((format!("game {tab:?}"), Route::Game(g), Some(tab), None));
    }
    for tab in [
        SettingsTab::General,
        SettingsTab::Appearance,
        SettingsTab::Protection,
        SettingsTab::CompatibilityLayers,
        SettingsTab::Integration,
        SettingsTab::Advanced,
        SettingsTab::About,
    ] {
        v.push((
            format!("settings {tab:?}"),
            Route::Settings(tab),
            None,
            None,
        ));
    }
    v
}

fn open(lib: LibraryView, route: Route, tab: Option<DetailsTab>, view: Option<ViewMode>) -> Rig {
    let mut r = rig_with(lib, route, Vec2::new(1280.0, 800.0), ThemeChoice::Dark);
    if let Some(v) = view {
        r.h.state_mut().state.library.view = v;
    }
    settle(&mut r);
    if let Some(t) = tab {
        r.h.state_mut().state.details.tab = t;
        settle(&mut r);
    }
    // Let toasts, debounces and fades finish as they would after a few idle seconds.
    r.clock.advance(Duration::from_secs(10));
    r.h.step();
    common::serve(&mut r);
    r.h.run();
    r
}

#[test]
fn every_route_is_idle_after_settling() {
    let lib = settled_library();
    let mut busy = Vec::new();
    for (name, route, tab, view) in routes(&lib) {
        let mut r = open(lib.clone(), route, tab, view);
        let (frames, shortest) = frames_over_window(&mut r);
        if frames != 0 {
            busy.push(format!("{name}: {frames} frames (every {shortest:?})"));
        }
    }
    assert!(busy.is_empty(), "repaints while idle: {busy:#?}");
}

#[test]
fn a_running_game_repaints_once_per_second() {
    let base = settled_library();
    let first = base.games[0].id;
    let lib = rebuild(&base, |g| {
        if g.id == first {
            g.activity = Activity::Running {
                since: common::now(),
            };
        }
    });
    let cases = [
        ("library grid", Route::Library, None, Some(ViewMode::Grid)),
        ("library list", Route::Library, None, Some(ViewMode::List)),
        ("game page", Route::Game(first), None, None),
    ];
    for (name, route, tab, view) in cases {
        let mut r = open(lib.clone(), route, tab, view);
        let (frames, shortest) = frames_over_window(&mut r);
        assert!(
            (1..=3).contains(&frames),
            "{name}: {frames} frames in {WINDOW:?}"
        );
        assert!(
            shortest >= Duration::from_millis(500),
            "{name}: repaint every {shortest:?}"
        );
    }
}
