//! Shared kittest helpers: a harness over [`FakeBackend`] and [`FakePicker`], the settled
//! sample library, and lookups.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use easyplay_core::clock::Timestamp;
use easyplay_core::ids::GameId;
use easyplay_core::model::{Activity, GameSummary};
use easyplay_testkit::FakeClock;
use easyplay_ui::bridge::{FakeBackend, LibraryView, UiBridge, library_from};
use easyplay_ui::input::picker::FakePicker;
use easyplay_ui::route::Route;
use easyplay_ui::textures::synthetic;
use easyplay_ui::theme::{self, Appearance, ThemeChoice};
use easyplay_ui::{EasyPlayApp, demo};
use egui::Vec2;
use egui_kittest::Harness;

/// The specification baseline.
pub fn now() -> Timestamp {
    demo::baseline_now()
}

/// Rebuild `lib` with `f` applied to every summary.
pub fn rebuild(lib: &LibraryView, mut f: impl FnMut(&mut GameSummary)) -> LibraryView {
    let games = lib
        .games
        .iter()
        .cloned()
        .map(|mut g| {
            f(&mut g);
            g
        })
        .collect();
    library_from(games, demo::sample_tags(), lib.revision.max(1), now())
}

/// The demo library without in-progress launches or covers, so the UI can settle.
pub fn settled_library() -> LibraryView {
    rebuild(&demo::library(now()), |g| {
        if matches!(
            g.activity,
            Activity::Launching(_) | Activity::Running { .. }
        ) {
            g.activity = Activity::Idle;
        }
        g.cover = None;
    })
}

/// A sample game's ID by name.
pub fn game_id(lib: &LibraryView, name: &str) -> GameId {
    lib.games.iter().find(|g| &*g.name == name).unwrap().id
}

/// A test rig.
pub struct Rig {
    /// The harness.
    pub h: Harness<'static, EasyPlayApp>,
    /// The backend.
    pub fake: FakeBackend,
    /// The picker.
    pub picker: FakePicker,
}

/// A rig at `start`, `size`, `choice`.
pub fn rig_with(lib: LibraryView, start: Route, size: Vec2, choice: ThemeChoice) -> Rig {
    let fake = FakeBackend::new(lib);
    rig_on(fake, start, size, choice)
}

/// A rig over an existing backend.
pub fn rig_on(fake: FakeBackend, start: Route, size: Vec2, choice: ThemeChoice) -> Rig {
    let picker = FakePicker::default();
    let clock = Arc::new(FakeClock::new(now()));
    let appearance = Appearance {
        theme: choice,
        ..Appearance::default()
    };
    let app = EasyPlayApp::new(
        UiBridge::new(Box::new(fake.clone())),
        clock,
        appearance,
        start,
        0,
    )
    .with_art(Box::new(|_| Box::new(synthetic::Inline::new())))
    .with_picker(Box::new(picker.clone()));
    let h = Harness::builder()
        .with_size(size)
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app);
    Rig { h, fake, picker }
}

/// The settled library at 1280 × 800, dark.
pub fn rig(start: Route) -> Rig {
    rig_with(
        settled_library(),
        start,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    )
}
