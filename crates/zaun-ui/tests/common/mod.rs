//! Shared kittest helpers: a harness over [`FakeBackend`] and [`FakePicker`], the settled
//! sample library, and lookups.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use egui::Vec2;
use egui_kittest::Harness;
use zaun_core::clock::Timestamp;
use zaun_core::ids::GameId;
use zaun_core::model::{Activity, GameSummary};
use zaun_testkit::FakeClock;
use zaun_ui::bridge::{FakeBackend, LibraryView, UiBridge, library_from};
use zaun_ui::input::picker::FakePicker;
use zaun_ui::route::Route;
use zaun_ui::textures::synthetic;
use zaun_ui::theme::{self, Appearance, ThemeChoice};
use zaun_ui::{ZaunApp, demo};

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

/// The settled library with the sample Steam games (ADR-0031): the preview's library,
/// without covers, with only the Steam game "Glass Garden" running.
pub fn settled_with_steam() -> LibraryView {
    rebuild(&demo::preview_library(now()), |g| {
        if !g.is_steam()
            && matches!(
                g.activity,
                Activity::Launching(_) | Activity::Running { .. }
            )
        {
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
    pub h: Harness<'static, ZaunApp>,
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
        ThemeChoice::Light => zaun_core::config::schema::Theme::Light,
        ThemeChoice::HighContrast => zaun_core::config::schema::Theme::HighContrast,
        _ => zaun_core::config::schema::Theme::Dark,
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
    f: impl FnOnce(ZaunApp) -> ZaunApp,
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
    f: impl FnOnce(ZaunApp) -> ZaunApp,
) -> Rig {
    let picker = FakePicker::default();
    let clock = Arc::new(FakeClock::new(now()));
    let app = ZaunApp::new(
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
    backend: Box<dyn zaun_ui::Backend>,
    start: Route,
    size: Vec2,
    choice: ThemeChoice,
) -> Harness<'static, ZaunApp> {
    let appearance = Appearance {
        theme: choice,
        ..Appearance::default()
    };
    let app = ZaunApp::new(
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
    use zaun_ui::bridge::{Command, QueryPayload, SteamAction, UiCommand, UiEvent};
    let lib = r.h.state().bridge().library().clone();
    let sent = r.fake.sent();
    r.fake.clear_sent();
    for c in &sent {
        let (id, payload) = match c {
            UiCommand::Core(Command::LoadGame { id, reply_to }) => (
                *reply_to,
                QueryPayload::Game(Box::new(demo::game_detail(lib.get(*id).unwrap(), now()))),
            ),
            UiCommand::Core(Command::QueryStats { request, reply_to }) => (
                *reply_to,
                QueryPayload::Stats(demo::stats(request, &lib, now())),
            ),
            UiCommand::Core(Command::AboutGame { id, reply_to }) => {
                let name = lib.get(*id).map(|g| g.name.to_string()).unwrap_or_default();
                (
                    *reply_to,
                    QueryPayload::About(Box::new(demo::game_about(&name))),
                )
            }
            UiCommand::Core(Command::MeasurePrefix { reply_to, .. }) => {
                (*reply_to, QueryPayload::PrefixSize(2_254_857_830))
            }
            UiCommand::Core(Command::ScanFolder { folder, reply_to }) => {
                (*reply_to, QueryPayload::Folder(demo::folder_scan(folder)))
            }
            UiCommand::Core(Command::FindSteamApp { id, term, reply_to }) => {
                let name = lib.get(*id).map(|g| g.name.to_string()).unwrap_or_default();
                (
                    *reply_to,
                    demo::steam_apps(term.as_deref().unwrap_or(&name)),
                )
            }
            UiCommand::Steam(SteamAction::Details { id, reply_to }) => {
                match lib.get(*id).and_then(demo::steam_detail) {
                    Some(d) => (*reply_to, QueryPayload::Steam(Box::new(d))),
                    None => continue,
                }
            }
            // Chat (ADR-0033): the canned history of the sample conversations.
            UiCommand::Core(Command::Chat(zaun_core::service::ChatCommand::LoadChat {
                chat,
                before,
                reply_to,
            })) => (
                *reply_to,
                QueryPayload::Chat(zaun_ui::demo_chat::page(*chat, *before, now())),
            ),
            // Steam is closed in these tests: the value is taken as it was sent.
            UiCommand::Steam(SteamAction::SetLaunchOptions {
                id,
                options,
                reply_to,
            }) => match lib.get(*id).and_then(demo::steam_detail) {
                Some(mut d) => {
                    d.launch_options = Some(options.clone());
                    (*reply_to, QueryPayload::Steam(Box::new(d)))
                }
                None => continue,
            },
            _ => continue,
        };
        r.fake.push_event(UiEvent::QueryResult { id, payload });
    }
    // Keep the non-query commands for the test to inspect.
    for c in sent {
        if !matches!(
            c,
            UiCommand::Core(Command::LoadGame { .. })
                | UiCommand::Core(Command::AboutGame { .. })
                | UiCommand::Core(Command::QueryStats { .. })
                | UiCommand::Core(Command::MeasurePrefix { .. })
                | UiCommand::Core(Command::ScanFolder { .. })
                | UiCommand::Core(Command::FindSteamApp { .. })
                | UiCommand::Steam(SteamAction::Details { .. })
                | UiCommand::Core(Command::Chat(
                    zaun_core::service::ChatCommand::LoadChat { .. }
                ))
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

/// A [`FakeBackend`] whose Chat snapshot the test sets (ADR-0033): every other call goes to
/// the fake, so its recorded commands and pushed events work as usual.
#[derive(Clone)]
pub struct ChatBackend {
    /// The fake underneath.
    pub fake: FakeBackend,
    chat: Arc<std::sync::Mutex<Arc<zaun_core::model::ChatSnapshot>>>,
}

impl ChatBackend {
    /// Over `fake`, with `chat` as the snapshot.
    pub fn new(fake: FakeBackend, chat: zaun_core::model::ChatSnapshot) -> Self {
        Self {
            fake,
            chat: Arc::new(std::sync::Mutex::new(Arc::new(chat))),
        }
    }

    /// Publish a new snapshot and announce it.
    pub fn publish_chat(&self, chat: zaun_core::model::ChatSnapshot) {
        *self.chat.lock().unwrap() = Arc::new(chat);
        self.fake.push_event(zaun_ui::bridge::UiEvent::ChatChanged);
    }
}

impl zaun_ui::Backend for ChatBackend {
    fn send(&self, cmd: zaun_ui::bridge::UiCommand) {
        self.fake.send(cmd);
    }
    fn try_event(&self) -> Option<zaun_ui::bridge::UiEvent> {
        self.fake.try_event()
    }
    fn library(&self) -> Arc<zaun_core::library::LibrarySnapshot> {
        self.fake.library()
    }
    fn preferences(&self) -> Arc<zaun_core::config::Preferences> {
        self.fake.preferences()
    }
    fn host(&self) -> Arc<zaun_ui::bridge::HostView> {
        self.fake.host()
    }
    fn runtimes(&self) -> Arc<zaun_ui::bridge::RuntimesView> {
        self.fake.runtimes()
    }
    fn assess(
        &self,
        game: Option<GameId>,
        name: &str,
        root: Option<&std::path::Path>,
        policy: &zaun_core::model::SandboxPolicy,
    ) -> zaun_ui::bridge::PolicyAssessment {
        self.fake.assess(game, name, root, policy)
    }
    fn preset_policy(
        &self,
        preset: zaun_core::model::BasePreset,
        policy: &zaun_core::model::SandboxPolicy,
    ) -> zaun_core::model::SandboxPolicy {
        self.fake.preset_policy(preset, policy)
    }
    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.fake.set_waker(waker);
    }
    fn is_preview(&self) -> bool {
        self.fake.is_preview()
    }
    fn diagnostics(&self) -> Arc<zaun_ui::bridge::Diagnostics> {
        self.fake.diagnostics()
    }
    fn chat(&self) -> Arc<zaun_core::model::ChatSnapshot> {
        self.chat.lock().unwrap().clone()
    }
    fn overlay(&self) -> zaun_core::chat::overlay::OverlayStatus {
        self.fake.overlay()
    }
}

/// A rig over a [`ChatBackend`] (its fake is the rig's `fake`), at `start`, `size`, with
/// the appearance `appearance`.
pub fn rig_chat(chat: &ChatBackend, start: Route, size: Vec2, appearance: Appearance) -> Rig {
    let picker = FakePicker::default();
    let clock = Arc::new(FakeClock::new(now()));
    let app = ZaunApp::new(
        UiBridge::new(Box::new(chat.clone())),
        clock.clone(),
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
    Rig {
        h,
        fake: chat.fake.clone(),
        picker,
        clock,
    }
}
