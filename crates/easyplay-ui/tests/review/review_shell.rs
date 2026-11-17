//! Review renders (throwaway): `cargo test -p easyplay-ui
//! --features review --test review review_shell:: -- --ignored
//! --test-threads=1`. Images land in `target/review/`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use common::*;
use easyplay_ui::route::Route;
use easyplay_ui::theme::ThemeChoice;
use egui::Vec2;

fn save(r: &mut Rig, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    let image = r.h.render().expect("render");
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

#[test]
#[ignore = "review render"]
fn library_dark() {
    let mut r = rig_with(
        settled_library(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    r.h.run_steps(30);
    save(&mut r, "library-dark");
}

#[test]
#[ignore = "review render"]
fn statistics_dark() {
    let mut r = rig_with(
        settled_library(),
        Route::Statistics,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    r.h.run_steps(30);
    save(&mut r, "shell-statistics-dark");
}

#[test]
#[ignore = "review render"]
fn running_dark() {
    let lib = demo_running();
    let mut r = rig_with(
        lib,
        Route::Statistics,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    r.h.run_steps(30);
    save(&mut r, "shell-running-dark");
}

fn demo_running() -> easyplay_ui::bridge::LibraryView {
    use easyplay_core::model::Activity;
    let base = settled_library();
    let started = now();
    rebuild(&base, |g| {
        if &*g.name == "Starfall Tactics" {
            g.activity = Activity::Running { since: started };
        }
    })
}

fn wide(route: Route, name: &str) {
    let mut r = rig_with(
        settled_library(),
        route,
        Vec2::new(1852.0, 1024.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    r.h.run_steps(40);
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn wide_pages() {
    use easyplay_ui::route::SettingsTab;
    wide(Route::Statistics, "shell-wide-statistics");
    wide(
        Route::Settings(SettingsTab::Appearance),
        "shell-wide-settings",
    );
    let lib = settled_library();
    wide(
        Route::Game(game_id(&lib, "Lantern Keep")),
        "shell-wide-game",
    );
    wide(Route::Library, "shell-wide-library");
}
