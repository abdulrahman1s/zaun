//! Review renders of the library (throwaway): `cargo test -p easyplay-ui --test
//! review_library -- --ignored --test-threads=1`. Images land in `target/review/`, named
//! `library-<reference name>.png`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use common::*;
use easyplay_core::ids::TagId;
use easyplay_core::library::PresetFilter;
use easyplay_core::model::{
    Activity, Availability, BasePreset, EntryKind, GameSummary, LaunchStateTag, NetworkAccess,
    Preset,
};
use easyplay_ui::bridge::{LibraryView, library_from};
use easyplay_ui::demo;
use easyplay_ui::route::Route;
use easyplay_ui::theme::ThemeChoice;
use easyplay_ui::vm::library::{Collection, FilterChip, Group, ViewMode};
use egui::{Pos2, Vec2};
use egui_kittest::kittest::Queryable;

const DAY: u64 = 24 * 3_600_000;

fn save(r: &mut Rig, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review");
    std::fs::create_dir_all(&dir).unwrap();
    let image = r.h.render().expect("render");
    image.save(dir.join(format!("library-{name}.png"))).unwrap();
}

/// The prototype's demo library: (title, preset, minutes, days since last played, tags,
/// favourite).
#[allow(clippy::type_complexity)]
const DEMO: [(&str, Preset, u64, Option<u64>, &[i64], bool); 16] = [
    (
        "Starfall Tactics",
        Preset::Online,
        1284,
        Some(0),
        &[2],
        false,
    ),
    ("Lantern Keep", Preset::Offline, 725, Some(1), &[1], true),
    ("Amber Sky", Preset::Standard, 74400, Some(1), &[], false),
    ("Low Tide Motel", Preset::Offline, 210, Some(2), &[], false),
    ("Tidewater", Preset::Online, 18, Some(2), &[], false),
    ("Moss & Iron", Preset::Offline, 252, Some(3), &[], false),
    (
        "Ninefold Arena",
        Preset::Online,
        24600,
        Some(4),
        &[2, 3],
        true,
    ),
    (
        "The Long Winter Road: Director's Edition",
        Preset::Offline,
        1350,
        Some(5),
        &[1],
        true,
    ),
    (
        "Hollow Bell",
        Preset::Custom {
            base: BasePreset::Offline,
        },
        900,
        Some(6),
        &[],
        false,
    ),
    ("Harbor Lights", Preset::Standard, 55, Some(9), &[], false),
    (
        "Clockwork Garden",
        Preset::Offline,
        122,
        Some(12),
        &[1],
        false,
    ),
    ("Driftwood", Preset::Offline, 40, Some(20), &[], false),
    ("Kestrel", Preset::Offline, 60, Some(33), &[], false),
    ("Ember Run", Preset::Standard, 180, Some(40), &[3], false),
    ("Paper Kingdoms", Preset::Online, 0, None, &[], false),
    ("Graywater", Preset::Online, 0, None, &[], false),
];

fn summary(template: &GameSummary, i: usize, title: &str) -> GameSummary {
    let mut g = template.clone();
    g.id = easyplay_core::ids::GameId::from_uuid(uuid::Uuid::from_u128(0x9000 + i as u128));
    g.name = title.into();
    g.sort_name = title.into();
    g.cover = None;
    g.icon = None;
    g.activity = Activity::Idle;
    g.availability = Availability::Present;
    g.kind = EntryKind::Game;
    g.flags = Default::default();
    g.hidden = false;
    g.added_at = now().saturating_sub(Duration::from_millis((20 + i as u64 * 6) * DAY));
    g
}

/// The first `n` games of the prototype's demo (with its installer when `n` is 16).
fn prototype(n: usize) -> LibraryView {
    let base = demo::library(now());
    let template = &base.games[0];
    let mut games: Vec<GameSummary> = DEMO
        .iter()
        .take(n)
        .enumerate()
        .map(|(i, (title, preset, minutes, days, tags, fav))| {
            let mut g = summary(template, i, title);
            g.preset = *preset;
            g.network = if preset.base() == BasePreset::Offline {
                NetworkAccess::None
            } else {
                NetworkAccess::Full
            };
            g.total_playtime_ms = minutes * 60_000;
            g.last_played_at = days.map(|d| now().saturating_sub(Duration::from_millis(d * DAY)));
            g.session_count = u32::from(days.is_some());
            g.tags = tags.iter().map(|t| TagId::new(*t)).collect();
            g.favorite = *fav;
            if *title == "Ember Run" {
                g.availability = Availability::Missing;
            }
            g
        })
        .collect();
    if n >= DEMO.len() {
        let mut g = summary(template, 99, "Setup Glassworks");
        g.kind = EntryKind::Installer;
        g.preset = Preset::Offline;
        g.total_playtime_ms = 0;
        g.last_played_at = None;
        g.session_count = 0;
        g.tags = Default::default();
        g.favorite = false;
        g.added_at = now().saturating_sub(Duration::from_millis(DAY));
        games.push(g);
    }
    library_from(games, demo::sample_tags(), 1, now())
}

/// The library of the first screenshot: one game.
fn one() -> LibraryView {
    let base = demo::library(now());
    let mut g = summary(&base.games[0], 0, "Grand Theft Auto San Andreas");
    g.preset = Preset::Offline;
    g.total_playtime_ms = 15 * 60_000;
    g.last_played_at = Some(now().saturating_sub(Duration::from_millis(DAY)));
    g.session_count = 1;
    g.tags = Default::default();
    g.favorite = false;
    library_from(vec![g], demo::sample_tags(), 1, now())
}

/// 240 games.
fn many() -> LibraryView {
    let base = demo::library(now());
    let a = [
        "Amber", "Hollow", "Northern", "Second", "Silent", "Copper", "Midnight", "Paper", "Salt",
        "Glass", "Iron", "Velvet", "Lantern", "Winter", "Ember", "Signal",
    ];
    let b = [
        "Circuit", "Orchard", "Station", "Kingdoms", "Relay", "Atlas", "Protocol", "Garden",
        "Frontier", "Archive", "Harvest", "Drift", "Foundry", "Parade", "Almanac",
    ];
    let mut games: Vec<GameSummary> = prototype(16).games.clone();
    let mut i = 0;
    while games.len() < 240 {
        let title = format!("{} {}", a[i % a.len()], b[(i / a.len() + i) % b.len()]);
        i += 1;
        if games.iter().any(|g| *g.name == *title) {
            continue;
        }
        let mut g = summary(&base.games[0], 100 + i, &title);
        g.total_playtime_ms = (i as u64 * 37 % 900) * 60_000;
        g.last_played_at = (i % 6 != 0)
            .then(|| now().saturating_sub(Duration::from_millis((7 + i as u64 * 3) * DAY)));
        g.tags = Default::default();
        g.favorite = i % 17 == 0;
        games.push(g);
    }
    library_from(games, demo::sample_tags(), 1, now())
}

fn rig_at(lib: LibraryView, w: f32, h: f32, theme: ThemeChoice) -> Rig {
    let mut r = rig_with(lib, Route::Library, Vec2::new(w, h), theme);
    settle(&mut r);
    r.h.run_steps(40);
    r
}

fn render(lib: LibraryView, name: &str) {
    let mut r = rig_at(lib, 1280.0, 800.0, ThemeChoice::Dark);
    save(&mut r, name);
}

/// A rig that keeps repainting (a spinner): stepped, not settled.
fn render_busy(lib: LibraryView, name: &str) {
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(60);
    save(&mut r, name);
}

fn id_of(r: &Rig, name: &str) -> easyplay_core::ids::GameId {
    game_id(r.h.state().bridge().library(), name)
}

fn pointer(r: &mut Rig, at: Pos2) {
    r.h.event(egui::Event::PointerMoved(at));
    r.h.run_steps(30);
}

#[test]
#[ignore = "review render"]
fn lib_sizes() {
    render(
        library_from(vec![], demo::sample_tags(), 1, now()),
        "lib-0-dark",
    );
    render(one(), "lib-1-dark");
    render(prototype(3), "lib-3-dark");
    render(prototype(5), "lib-5-dark");
    render(prototype(6), "lib-6-dark");
    render(prototype(16), "lib-16-dark");
    render(many(), "lib-240-dark");
}

#[test]
#[ignore = "review render"]
fn lib_themes() {
    for (theme, suffix) in [
        (ThemeChoice::Light, "light"),
        (ThemeChoice::HighContrast, "hc"),
    ] {
        let mut r = rig_at(prototype(16), 1280.0, 800.0, theme);
        save(&mut r, &format!("lib-16-{suffix}"));
        let mut r = rig_at(one(), 1280.0, 800.0, theme);
        save(&mut r, &format!("lib-1-{suffix}"));
        let mut r = rig_at(prototype(3), 1280.0, 800.0, theme);
        save(&mut r, &format!("lib-3-{suffix}"));
    }
}

#[test]
#[ignore = "review render"]
fn lib_hover() {
    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    pointer(&mut r, Pos2::new(120.0, 240.0));
    save(&mut r, "lib-16-hover");
    pointer(&mut r, Pos2::new(530.0, 240.0));
    save(&mut r, "lib-16-hover-fav");
    pointer(&mut r, Pos2::new(1150.0, 240.0));
    save(&mut r, "lib-16-hover-last");
}

#[test]
#[ignore = "review render"]
fn lib_states() {
    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    let ids = ["Starfall Tactics", "Amber Sky", "Tidewater"].map(|n| id_of(&r, n));
    r.h.state_mut().state.library.selection = ids.to_vec();
    r.h.run_steps(30);
    save(&mut r, "lib-16-selected");

    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    r.h.state_mut()
        .state
        .library
        .set_collection(Collection::Favorites);
    r.h.run_steps(30);
    save(&mut r, "lib-16-favourites");
    r.h.state_mut()
        .state
        .library
        .set_collection(Collection::NeverPlayed);
    r.h.run_steps(30);
    save(&mut r, "lib-16-never");

    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    {
        let lib = &mut r.h.state_mut().state.library;
        lib.toggle_filter(FilterChip::Preset(PresetFilter::Online));
        lib.toggle_filter(FilterChip::Tag(TagId::new(1)));
    }
    r.h.run_steps(30);
    save(&mut r, "lib-16-filtered");

    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    r.h.state_mut().state.library.query = "zzz".into();
    r.h.run_steps(30);
    save(&mut r, "lib-16-nomatch");
    r.h.state_mut().state.library.query = "ar".into();
    r.h.run_steps(30);
    save(&mut r, "lib-16-search");

    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    r.h.state_mut().state.library.group = Group::Protection;
    r.h.run_steps(30);
    save(&mut r, "lib-16-grouped");
}

#[test]
#[ignore = "review render"]
fn lib_running() {
    let starting = rebuild(&prototype(16), |g| {
        if &*g.name == "Starfall Tactics" {
            g.activity = Activity::Launching(LaunchStateTag::Starting);
        }
    });
    render_busy(starting, "lib-16-starting");
    let running = rebuild(&prototype(16), |g| {
        if &*g.name == "Starfall Tactics" {
            g.activity = Activity::Running {
                since: now().saturating_sub(Duration::from_secs(1)),
            };
        }
    });
    render_busy(running, "lib-16-running");
}

#[test]
#[ignore = "review render"]
fn lib_loading() {
    let lib = library_from(vec![], demo::sample_tags(), 0, now());
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(20);
    save(&mut r, "lib-loading");
}

#[test]
#[ignore = "review render"]
fn lib_narrow() {
    let mut r = rig_at(prototype(16), 840.0, 700.0, ThemeChoice::Dark);
    save(&mut r, "lib-16-840");
    let mut r = rig_at(prototype(16), 600.0, 700.0, ThemeChoice::Dark);
    save(&mut r, "lib-16-600");
    for (w, name) in [(840.0, "list-16-840"), (600.0, "list-16-600")] {
        let mut r = rig_at(prototype(16), w, 700.0, ThemeChoice::Dark);
        r.h.state_mut().state.library.view = ViewMode::List;
        r.h.run_steps(40);
        save(&mut r, name);
    }
}

#[test]
#[ignore = "review render"]
fn list_views() {
    for (theme, name) in [
        (ThemeChoice::Dark, "list-16-dark"),
        (ThemeChoice::Light, "list-16-light"),
    ] {
        let mut r = rig_at(prototype(16), 1280.0, 800.0, theme);
        r.h.state_mut().state.library.view = ViewMode::List;
        r.h.run_steps(40);
        save(&mut r, name);
    }
    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    r.h.state_mut().state.library.view = ViewMode::List;
    r.h.run_steps(40);
    pointer(&mut r, Pos2::new(600.0, 156.0));
    save(&mut r, "list-16-hover");
}

#[test]
#[ignore = "review render"]
fn menus() {
    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    r.h.get_by_label("Recently played").click();
    r.h.run_steps(30);
    save(&mut r, "menu-sort");

    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    r.h.get_by_label("Filter").click();
    r.h.run_steps(30);
    save(&mut r, "menu-filter");

    let mut r = rig_at(prototype(16), 1280.0, 800.0, ThemeChoice::Dark);
    let at = Pos2::new(300.0, 180.0);
    pointer(&mut r, at);
    for pressed in [true, false] {
        r.h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        r.h.run_steps(2);
    }
    r.h.run_steps(30);
    save(&mut r, "menu-game");
}

/// A rig stepping at 60 frames a second (to watch motion frame by frame).
fn rig_60(lib: LibraryView, w: f32, h: f32) -> Rig {
    use std::sync::Arc;
    let fake = easyplay_ui::bridge::FakeBackend::new(lib);
    let picker = easyplay_ui::input::picker::FakePicker::default();
    let clock = Arc::new(easyplay_testkit::FakeClock::new(now()));
    let appearance = easyplay_ui::theme::Appearance {
        theme: ThemeChoice::Dark,
        ..Default::default()
    };
    let app = easyplay_ui::EasyPlayApp::new(
        easyplay_ui::bridge::UiBridge::new(Box::new(fake.clone())),
        clock.clone(),
        appearance,
        Route::Library,
        0,
    )
    .with_art(Box::new(|_| {
        Box::new(easyplay_ui::textures::synthetic::Inline::new())
    }))
    .with_picker(Box::new(picker.clone()));
    let h = egui_kittest::Harness::builder()
        .with_size(Vec2::new(w, h))
        .with_step_dt(1.0 / 60.0)
        .with_context_setup(move |ctx| easyplay_ui::theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app);
    Rig {
        h,
        fake,
        picker,
        clock,
    }
}

/// The prototype's library with artwork.
fn with_art(n: usize) -> LibraryView {
    let base = demo::library(now());
    let arts: Vec<_> = base.games.iter().filter_map(|g| g.cover).collect();
    let mut i = 0;
    rebuild(&prototype(n), |g| {
        g.cover = arts.get(i % arts.len().max(1)).copied();
        i += 1;
    })
}

/// Covers ↔ list, frame by frame: the path of one cover, and renders mid-flight.
fn switch_frames(from: ViewMode, to: ViewMode, name: &str, n: usize) {
    use easyplay_ui::screens::library::grid::last_drawn;
    let mut r = rig_60(with_art(n), 1280.0, 800.0);
    r.h.state_mut().state.library.view = from;
    r.h.run_steps(120);
    let id = id_of(&r, "Amber Sky");
    let label = if to == ViewMode::Grid {
        "Covers"
    } else {
        "List"
    };
    r.h.get_by_label(label).click();
    for f in 0..30 {
        r.h.step();
        eprintln!("{name} frame {f}: {:?}", last_drawn(&r.h.ctx, id));
        if f == 8 {
            save(&mut r, &format!("{name}-mid"));
        }
    }
    r.h.run_steps(60);
    save(&mut r, &format!("{name}-end"));
}

#[test]
#[ignore = "review render"]
fn switch_motion() {
    // 13 games: the list fits the page and the wall does not, so the page's scroll bar
    // comes or goes during the switch.
    switch_frames(ViewMode::Grid, ViewMode::List, "switch-to-list", 13);
    switch_frames(ViewMode::List, ViewMode::Grid, "switch-to-covers", 13);
}
