//! Review renders of the game page (throwaway): `cargo test -p easyplay-ui
//! --features review --test review review_game:: -- --ignored
//! --test-threads=1`. Images land in `target/review/game-*.png`, to
//! compare with the prototype's screenshots of the same names.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use std::time::Duration;

use common::*;
use easyplay_core::model::{Activity, Availability};
use easyplay_ui::route::Route;
use easyplay_ui::theme::ThemeChoice;
use easyplay_ui::vm::details::DetailsTab;
use egui::Vec2;

fn save(r: &mut Rig, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    let image = r.h.render().expect("render");
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

/// The page of `name` on `tab`, for a state that keeps repainting (a spinner).
fn busy_page(lib: easyplay_ui::bridge::LibraryView, name: &str) -> Rig {
    let id = game_id(&lib, name);
    let mut r = rig_with(lib, Route::Game(id), wide(), ThemeChoice::Dark);
    for _ in 0..6 {
        r.h.run_steps(20);
        serve(&mut r);
    }
    r.h.run_steps(60);
    r
}

/// The page of `name`, settled, on `tab`.
fn page(
    lib: easyplay_ui::bridge::LibraryView,
    name: &str,
    tab: DetailsTab,
    size: Vec2,
    choice: ThemeChoice,
) -> Rig {
    let id = game_id(&lib, name);
    let mut r = rig_with(lib, Route::Game(id), size, choice);
    settle(&mut r);
    r.h.state_mut().state.details.tab = tab;
    for _ in 0..4 {
        r.h.run_steps(20);
        serve(&mut r);
    }
    r.h.run_steps(60);
    r
}

fn wide() -> Vec2 {
    Vec2::new(1280.0, 800.0)
}

fn shot(name: &str, game: &str, tab: DetailsTab) {
    let mut r = page(settled_library(), game, tab, wide(), ThemeChoice::Dark);
    save(&mut r, name);
}

/// Scroll the page's column by `dy` points.
fn scroll(r: &mut Rig, dy: f32) {
    r.h.input_mut()
        .events
        .push(egui::Event::PointerMoved(egui::pos2(700.0, 500.0)));
    r.h.run_steps(2);
    for _ in 0..(dy / 50.0) as usize {
        r.h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -50.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
        });
        r.h.run_steps(1);
    }
    r.h.run_steps(60);
}

#[test]
#[ignore = "review render"]
fn game_overview() {
    shot("game-overview", "Lantern Keep", DetailsTab::Overview);
}

#[test]
#[ignore = "review render"]
fn game_protection() {
    shot("game-protection", "Lantern Keep", DetailsTab::Protection);
}

#[test]
#[ignore = "review render"]
fn game_compatibility() {
    shot(
        "game-compatibility",
        "Lantern Keep",
        DetailsTab::Compatibility,
    );
}

#[test]
#[ignore = "review render"]
fn game_launch() {
    shot("game-launch", "Lantern Keep", DetailsTab::LaunchOptions);
}

#[test]
#[ignore = "review render"]
fn game_activity() {
    shot("game-activity", "Lantern Keep", DetailsTab::Activity);
}

#[test]
#[ignore = "review render"]
fn game_logs() {
    shot("game-logs", "Lantern Keep", DetailsTab::Logs);
}

#[test]
#[ignore = "review render"]
fn game_overview_scrolled() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    scroll(&mut r, 350.0);
    save(&mut r, "game-overview-scrolled");
}

#[test]
#[ignore = "review render"]
fn game_activity_scrolled() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Activity,
        wide(),
        ThemeChoice::Dark,
    );
    scroll(&mut r, 350.0);
    save(&mut r, "game-activity-scrolled");
}

#[test]
#[ignore = "review render"]
fn game_protection_custom() {
    shot(
        "game-protection-custom",
        "Quiet Orbit",
        DetailsTab::Protection,
    );
}

#[test]
#[ignore = "review render"]
fn game_overview_light() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Light,
    );
    save(&mut r, "game-overview-light");
}

#[test]
#[ignore = "review render"]
fn game_protection_light() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Protection,
        wide(),
        ThemeChoice::Light,
    );
    save(&mut r, "game-protection-light");
}

#[test]
#[ignore = "review render"]
fn game_overview_hc() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::HighContrast,
    );
    save(&mut r, "game-overview-hc");
}

#[test]
#[ignore = "review render"]
fn game_problem() {
    let lib = settled_library();
    let id = game_id(&lib, "Tidewater");
    let mut r = page(
        lib,
        "Tidewater",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    r.h.state_mut()
        .state
        .launches
        .failures
        .insert(id, easyplay_ui::demo::launch_failure(id, "Tidewater"));
    r.h.run_steps(60);
    save(&mut r, "game-problem");
}

#[test]
#[ignore = "review render"]
fn game_missing() {
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Ember Run" {
            g.availability = Availability::Missing;
        }
    });
    let mut r = page(
        lib,
        "Ember Run",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    save(&mut r, "game-missing");
}

#[test]
#[ignore = "review render"]
fn game_lingering() {
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Moss & Iron" {
            g.activity = Activity::Lingering;
        }
    });
    let mut r = page(
        lib,
        "Moss & Iron",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    save(&mut r, "game-lingering");
}

#[test]
#[ignore = "review render"]
fn game_never() {
    shot("game-never", "Vale of Cinders", DetailsTab::Overview);
}

#[test]
#[ignore = "review render"]
fn game_never_activity() {
    shot(
        "game-never-activity",
        "Vale of Cinders",
        DetailsTab::Activity,
    );
}

#[test]
#[ignore = "review render"]
fn game_never_logs() {
    shot("game-never-logs", "Vale of Cinders", DetailsTab::Logs);
}

#[test]
#[ignore = "review render"]
fn game_long_title() {
    shot(
        "game-long-title",
        "The Long Winter Road: Director's Edition",
        DetailsTab::Overview,
    );
}

#[test]
#[ignore = "review render"]
fn game_running() {
    let since = now().saturating_sub(Duration::from_secs(3));
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Starfall Tactics" {
            g.activity = Activity::Running { since };
        }
    });
    let mut r = page(
        lib,
        "Starfall Tactics",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    save(&mut r, "game-running");
}

#[test]
#[ignore = "review render"]
fn game_blocked() {
    // EasyPlay runs games side by side: Play stays Play while another game runs.
    let since = now().saturating_sub(Duration::from_secs(3));
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Starfall Tactics" {
            g.activity = Activity::Running { since };
        }
    });
    let mut r = page(
        lib,
        "Lantern Keep",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    save(&mut r, "game-blocked");
}

#[test]
#[ignore = "review render"]
fn game_overview_840() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Overview,
        Vec2::new(840.0, 700.0),
        ThemeChoice::Dark,
    );
    save(&mut r, "game-overview-840");
}

#[test]
#[ignore = "review render"]
fn game_protection_840() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Protection,
        Vec2::new(840.0, 700.0),
        ThemeChoice::Dark,
    );
    save(&mut r, "game-protection-840");
}

#[test]
#[ignore = "review render"]
fn game_overview_600() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Overview,
        Vec2::new(600.0, 700.0),
        ThemeChoice::Dark,
    );
    save(&mut r, "game-overview-600");
}

/// A log of `n` lines (some warnings and errors) for `game`, already loaded.
fn with_log(r: &mut Rig, name: &str, n: usize, wrap: bool) {
    use easyplay_core::clock::Clock;
    use easyplay_core::ids::SessionId;
    use easyplay_ui::bridge::{LogChunk, LogFile};
    use easyplay_ui::vm::debug::LogViewerVm;
    let lib = r.h.state().bridge().library().clone();
    let g = game_id(&lib, name);
    let mut s = easyplay_testkit::fixtures::games::session(
        &easyplay_testkit::fixtures::games::game(1, 1),
        1,
        1,
        now(),
    );
    let sid = SessionId::from_uuid(uuid::Uuid::from_u128(77));
    s.id = sid;
    s.game_id = Some(g);
    let mut text = String::new();
    for i in 0..n {
        let line = match i % 7 {
            3 => format!(
                "[+{}.{:03}] err:module:import_dll Library d3d11.dll (which is needed by L\"Z:\\\\Games\\\\Lantern Keep\\\\LanternKeep.exe\") not found",
                i,
                i * 37 % 1000
            ),
            5 => format!(
                "[+{}.{:03}] WARN: shader cache is cold, the first minutes may stutter",
                i,
                i * 37 % 1000
            ),
            _ => format!(
                "[+{}.{:03}] INFO: umu-launcher version 1.4.4, step {i}",
                i,
                i * 37 % 1000
            ),
        };
        text.push_str(&line);
        text.push('\n');
    }
    let mut l = LogViewerVm {
        game: Some(g),
        sessions: vec![s],
        listed: true,
        ..LogViewerVm::default()
    };
    l.open(sid, LogFile::Game);
    l.on_chunk(LogChunk {
        session: sid,
        file: LogFile::Game,
        offset: 0,
        bytes: text.as_bytes().to_vec(),
        size: text.len() as u64,
        missing: false,
    });
    l.polled = Some(r.clock.mono());
    l.follow = false;
    l.wrap = wrap;
    l.level = easyplay_core::diagnostics::log_line::LineLevel::Debug;
    r.h.state_mut().state.debug.logs = Some(l);
    r.h.run_steps(30);
}

#[test]
#[ignore = "review render"]
fn game_logs_lines() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Logs,
        wide(),
        ThemeChoice::Dark,
    );
    with_log(&mut r, "Lantern Keep", 34, false);
    save(&mut r, "game-logs-lines");
    with_log(&mut r, "Lantern Keep", 34, true);
    save(&mut r, "game-logs-lines-wrapped");
}

#[test]
#[ignore = "review render"]
fn game_logs_sheet() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    with_log(&mut r, "Lantern Keep", 34, false);
    r.h.state_mut()
        .state
        .nav
        .open(easyplay_ui::route::Overlay::Logs);
    r.h.run_steps(60);
    save(&mut r, "game-logs-sheet");
}

#[test]
#[ignore = "review render"]
fn game_ended_early() {
    let lib = settled_library();
    let id = game_id(&lib, "Lantern Keep");
    let mut r = page(
        lib,
        "Lantern Keep",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    r.h.state_mut()
        .state
        .launches
        .ended_early
        .insert(id, easyplay_core::model::EndReason::Crashed { signal: 11 });
    r.h.run_steps(60);
    save(&mut r, "game-ended-early");
}

#[test]
#[ignore = "review render"]
fn game_compatibility_bottom() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Compatibility,
        wide(),
        ThemeChoice::Dark,
    );
    scroll(&mut r, 1500.0);
    save(&mut r, "game-compatibility-bottom");
}

#[test]
#[ignore = "review render"]
fn game_launch_bottom() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::LaunchOptions,
        wide(),
        ThemeChoice::Dark,
    );
    let exe = r.h.state().state.details.record().unwrap().game.executables[0].id;
    r.h.state_mut().state.details.program_edit = Some(easyplay_ui::vm::details::ProgramEdit {
        exe,
        field: easyplay_ui::vm::details::ProgramField::Args,
        text: "-dx11".into(),
    });
    r.h.run_steps(30);
    save(&mut r, "game-launch-editing");
    scroll(&mut r, 1500.0);
    save(&mut r, "game-launch-bottom");
}

#[test]
#[ignore = "review render"]
fn game_launching() {
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Lantern Keep" {
            g.activity = Activity::Launching(easyplay_core::model::LaunchStateTag::Preparing);
        }
    });
    let mut r = busy_page(lib, "Lantern Keep");
    save(&mut r, "game-launching");
}

#[test]
#[ignore = "review render"]
fn game_stopping() {
    let lib = rebuild(&settled_library(), |g| {
        if &*g.name == "Lantern Keep" {
            g.activity = Activity::Stopping;
        }
    });
    let mut r = busy_page(lib, "Lantern Keep");
    save(&mut r, "game-stopping");
}

#[test]
#[ignore = "review render"]
fn game_protection_dirty() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Protection,
        wide(),
        ThemeChoice::Dark,
    );
    {
        let e = r.h.state_mut().state.details.protection.as_mut().unwrap();
        let mut p = e.draft.permissions();
        p.insert(easyplay_core::model::Permissions::INTERNET);
        e.draft.set_permissions(p);
        e.reclassify();
    }
    r.h.run_steps(60);
    save(&mut r, "game-protection-dirty");
}

/// The note of a game that carries developer-only options, closed and open.
#[test]
#[ignore = "review render"]
fn game_advanced_note() {
    let mut r = page(
        settled_library(),
        "Lantern Keep",
        DetailsTab::Overview,
        wide(),
        ThemeChoice::Dark,
    );
    if let easyplay_ui::vm::AsyncValue::Ready(d) = &mut r.h.state_mut().state.details.detail {
        d.game.launch.compat.wine_debug = Some("+relay".into());
    }
    r.h.run_steps(20);
    save(&mut r, "game-advanced-note");
    r.h.state_mut().state.debug.review_open = true;
    r.h.run_steps(20);
    save(&mut r, "game-advanced-note-open");
}
