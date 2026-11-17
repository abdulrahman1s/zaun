//! Review renders of the Steam screens (ADR-0031), throwaway: `cargo test -p easyplay-ui
//! --test review_steam -- --ignored --test-threads=1`. Images land in
//! `target/review/steam_*.png`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;

use common::*;
use easyplay_core::model::{GameOrigin, SteamDownload, SteamUpdate};
use easyplay_ui::bridge::Backend;
use easyplay_ui::route::{Route, SettingsTab};
use easyplay_ui::theme::ThemeChoice;
use easyplay_ui::vm::details::SteamTab;
use easyplay_ui::vm::library::ViewMode;
use egui::Vec2;
use egui_kittest::kittest::Queryable;

fn save(r: &mut Rig, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    let image = r.h.render().expect("render");
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

fn size() -> Vec2 {
    Vec2::new(1280.0, 800.0)
}

fn settle_steps(r: &mut Rig) {
    for _ in 0..4 {
        r.h.run_steps(20);
        serve(r);
    }
    r.h.run_steps(60);
}

fn library(name: &str, view: ViewMode, choice: ThemeChoice) {
    let mut r = rig_with(settled_with_steam(), Route::Library, size(), choice);
    r.h.state_mut().state.library.view = view;
    settle_steps(&mut r);
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn steam_wall() {
    library("steam_wall_dark", ViewMode::Grid, ThemeChoice::Dark);
    library("steam_wall_light", ViewMode::Grid, ThemeChoice::Light);
}

#[test]
#[ignore = "review render"]
fn steam_wall_pointed() {
    let lib = settled_with_steam();
    let mut r = rig_with(lib, Route::Library, size(), ThemeChoice::Dark);
    settle_steps(&mut r);
    // Point at the running Steam game's cover (the first: played today).
    let rect =
        r.h.get_by_label_contains("Glass Garden. Run by Steam")
            .rect();
    r.h.input_mut()
        .events
        .push(egui::Event::PointerMoved(rect.center()));
    r.h.run_steps(30);
    save(&mut r, "steam_wall_pointed_dark");
}

#[test]
#[ignore = "review render"]
fn steam_list() {
    library("steam_list_dark", ViewMode::List, ThemeChoice::Dark);
}

#[test]
#[ignore = "review render"]
fn steam_scope() {
    let mut r = rig_with(
        settled_with_steam(),
        Route::Library,
        size(),
        ThemeChoice::Dark,
    );
    r.h.state_mut().state.library.view = ViewMode::List;
    r.h.state_mut()
        .state
        .library
        .set_collection(easyplay_ui::vm::library::Collection::Steam);
    settle_steps(&mut r);
    save(&mut r, "steam_scope_list_dark");
}

#[test]
#[ignore = "review render"]
fn steam_menu() {
    let lib = settled_with_steam();
    let id = game_id(&lib, "Ridgeline Rally");
    let mut r = rig_with(lib, Route::Library, size(), ThemeChoice::Dark);
    settle_steps(&mut r);
    r.h.state_mut().state.library.menu = Some((id, egui::pos2(500.0, 200.0)));
    r.h.run_steps(30);
    save(&mut r, "steam_menu_dark");
}

fn page(lib: easyplay_ui::bridge::LibraryView, game: &str, tab: SteamTab, name: &str) {
    let id = game_id(&lib, game);
    let mut r = rig_with(lib, Route::Game(id), size(), ThemeChoice::Dark);
    settle_steps(&mut r);
    r.h.state_mut().state.details.steam_tab = tab;
    settle_steps(&mut r);
    save(&mut r, name);
}

#[test]
#[ignore = "review render"]
fn steam_pages() {
    page(
        settled_with_steam(),
        "Ridgeline Rally",
        SteamTab::Overview,
        "steam_overview_dark",
    );
    page(
        settled_with_steam(),
        "Ridgeline Rally",
        SteamTab::Storage,
        "steam_storage_dark",
    );
    page(
        settled_with_steam(),
        "Brass Lighthouse",
        SteamTab::Overview,
        "steam_native_dark",
    );
    page(
        settled_with_steam(),
        "Glass Garden",
        SteamTab::Storage,
        "steam_running_storage_dark",
    );
}

#[test]
#[ignore = "review render"]
fn steam_update_note() {
    let lib = rebuild(&settled_with_steam(), |g| {
        if &*g.name == "Ridgeline Rally"
            && let GameOrigin::Steam(f) = &g.origin
        {
            let mut f = (**f).clone();
            f.update = SteamUpdate::Updating;
            f.download = Some(SteamDownload {
                done: 820_000_000,
                total: 1_400_000_000,
            });
            f.files_missing = true;
            g.origin = GameOrigin::Steam(Arc::new(f));
        }
    });
    page(
        lib,
        "Ridgeline Rally",
        SteamTab::Overview,
        "steam_update_note_dark",
    );
}

#[test]
#[ignore = "review render"]
fn steam_settings_general() {
    let mut r = rig_with(
        settled_library(),
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Dark,
    );
    settle_steps(&mut r);
    save(&mut r, "steam_settings_general_dark");
}

#[test]
#[ignore = "review render"]
fn steam_statistics() {
    let mut r = rig_with(
        settled_with_steam(),
        Route::Statistics,
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Dark,
    );
    settle_steps(&mut r);
    save(&mut r, "steam_statistics_dark");
}

#[test]
#[ignore = "review render"]
fn steam_page_narrow() {
    let lib = settled_with_steam();
    let id = game_id(&lib, "Ridgeline Rally");
    let mut r = rig_with(
        lib,
        Route::Game(id),
        Vec2::new(600.0, 800.0),
        ThemeChoice::Dark,
    );
    settle_steps(&mut r);
    save(&mut r, "steam_overview_600_dark");
}

fn save_h(h: &mut egui_kittest::Harness<'static, easyplay_ui::EasyPlayApp>, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    let image = h.render().expect("render");
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

#[test]
#[ignore = "review render"]
fn steam_preview() {
    let backend = easyplay_ui::demo::DemoBackend::new(now());
    let mut h = rig_backend(
        Box::new(backend.clone()),
        Route::Library,
        size(),
        ThemeChoice::Dark,
    );
    h.run_steps(60);
    save_h(&mut h, "steam_preview_wall_dark");
    let id = game_id(&backend.library(), "Ridgeline Rally");
    h.state_mut().state.nav.replace(Route::Game(id));
    h.run_steps(80);
    save_h(&mut h, "steam_preview_page_dark");
}

#[test]
#[ignore = "review render"]
fn steam_filter_menu() {
    let mut r = rig_with(
        settled_with_steam(),
        Route::Library,
        size(),
        ThemeChoice::Dark,
    );
    settle_steps(&mut r);
    r.h.get_by_label("Filter").click();
    r.h.run_steps(30);
    save(&mut r, "steam_filter_menu_dark");
}
