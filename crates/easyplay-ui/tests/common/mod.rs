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
    /// The clock.
    pub clock: Arc<FakeClock>,
}

/// A rig at `start`, `size`, `choice`.
pub fn rig_with(lib: LibraryView, start: Route, size: Vec2, choice: ThemeChoice) -> Rig {
    let fake = FakeBackend::new(lib);
    rig_on(fake, start, size, choice)
}

/// A rig over an existing backend.
pub fn rig_on(fake: FakeBackend, start: Route, size: Vec2, choice: ThemeChoice) -> Rig {
    rig_app(fake, start, size, choice, |a| a)
}

/// A rig at UI scale `scale` showing a window of `logical` points (the harness keeps the
/// window size in points; the zoom factor scales the pixels, as on a high-density screen).
/// The preferences carry the same scale, so a preference reload keeps it.
pub fn rig_scaled(start: Route, logical: Vec2, scale: f32, choice: ThemeChoice) -> Rig {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.appearance.ui_scale = f64::from(scale);
    prefs.appearance.theme = match choice {
        ThemeChoice::Light => easyplay_core::config::schema::Theme::Light,
        ThemeChoice::HighContrast => easyplay_core::config::schema::Theme::HighContrast,
        _ => easyplay_core::config::schema::Theme::Dark,
    };
    fake.publish_prefs(prefs);
    rig_appearance(
        fake,
        start,
        logical,
        Appearance {
            theme: choice,
            ui_scale: scale,
            ..Appearance::default()
        },
        |a| a,
    )
}

/// A rig whose app is adjusted by `f` before the first frame (quick launch).
pub fn rig_app(
    fake: FakeBackend,
    start: Route,
    size: Vec2,
    choice: ThemeChoice,
    f: impl FnOnce(EasyPlayApp) -> EasyPlayApp,
) -> Rig {
    let appearance = Appearance {
        theme: choice,
        ..Appearance::default()
    };
    rig_appearance(fake, start, size, appearance, f)
}

/// A rig with a full appearance.
pub fn rig_appearance(
    fake: FakeBackend,
    start: Route,
    size: Vec2,
    appearance: Appearance,
    f: impl FnOnce(EasyPlayApp) -> EasyPlayApp,
) -> Rig {
    let picker = FakePicker::default();
    let clock = Arc::new(FakeClock::new(now()));
    let app = EasyPlayApp::new(
        UiBridge::new(Box::new(fake.clone())),
        clock.clone(),
        appearance,
        start,
        0,
    )
    .with_art(Box::new(|_| Box::new(synthetic::Inline::new())))
    .with_picker(Box::new(picker.clone()));
    let app = f(app);
    let h = Harness::builder()
        .with_size(size)
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app);
    Rig {
        h,
        fake,
        picker,
        clock,
    }
}

/// A harness over any backend (no picker, synthetic artwork).
pub fn rig_backend(
    backend: Box<dyn easyplay_ui::Backend>,
    start: Route,
    size: Vec2,
    choice: ThemeChoice,
) -> Harness<'static, EasyPlayApp> {
    let appearance = Appearance {
        theme: choice,
        ..Appearance::default()
    };
    let app = EasyPlayApp::new(
        UiBridge::new(backend),
        Arc::new(FakeClock::new(now())),
        appearance,
        start,
        0,
    )
    .with_art(Box::new(|_| Box::new(synthetic::Inline::new())));
    Harness::builder()
        .with_size(size)
        .with_context_setup(move |ctx| theme::install(ctx, &appearance))
        .build_eframe(move |_cc| app)
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

/// Answer every query the UI has sent and not yet seen answered, with demo data.
pub fn serve(r: &mut Rig) {
    use easyplay_ui::bridge::{QueryPayload, UiCommand, UiEvent};
    let lib = r.h.state().bridge().library().clone();
    let sent = r.fake.sent();
    r.fake.clear_sent();
    for c in &sent {
        let (id, payload) = match c {
            UiCommand::LoadGame { id, reply_to } => (
                *reply_to,
                QueryPayload::Game(Box::new(demo::game_detail(lib.get(*id).unwrap(), now()))),
            ),
            UiCommand::QueryStats { request, reply_to } => (
                *reply_to,
                QueryPayload::Stats(demo::stats(request, &lib, now())),
            ),
            UiCommand::AboutGame { id, reply_to } => {
                let name = lib.get(*id).map(|g| g.name.to_string()).unwrap_or_default();
                (
                    *reply_to,
                    QueryPayload::About(Box::new(demo::game_about(&name))),
                )
            }
            UiCommand::MeasurePrefix { reply_to, .. } => {
                (*reply_to, QueryPayload::PrefixSize(2_254_857_830))
            }
            UiCommand::ScanFolder { folder, reply_to } => {
                (*reply_to, QueryPayload::Folder(demo::folder_scan(folder)))
            }
            UiCommand::FindSteamApp { id, term, reply_to } => {
                let name = lib.get(*id).map(|g| g.name.to_string()).unwrap_or_default();
                (
                    *reply_to,
                    demo::steam_apps(term.as_deref().unwrap_or(&name)),
                )
            }
            _ => continue,
        };
        r.fake.push_event(UiEvent::QueryResult { id, payload });
    }
    // Keep the non-query commands for the test to inspect.
    for c in sent {
        if !matches!(
            c,
            UiCommand::LoadGame { .. }
                | UiCommand::AboutGame { .. }
                | UiCommand::QueryStats { .. }
                | UiCommand::MeasurePrefix { .. }
                | UiCommand::ScanFolder { .. }
                | UiCommand::FindSteamApp { .. }
        ) {
            r.fake.record(c);
        }
    }
}

/// Run, answer queries, run again (twice, for queries sent in response).
pub fn settle(r: &mut Rig) {
    for _ in 0..3 {
        r.h.run_steps(2);
        serve(r);
    }
    r.h.run();
}
