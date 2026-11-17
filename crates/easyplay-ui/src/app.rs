//! `EasyPlayApp` implementing `eframe::App`: `logic` (drain events, route them to the
//! view-models, collect picker results), `ui` (shell + current route + overlays),
//! `raw_input_hook` (drops arrive through egui's input), `on_exit` (save geometry). See
//! MODULES §8 and task UI-001.
//!
//! [`EasyPlayApp::frame`] is the whole per-frame UI; the eframe `ui` hook and the kittest
//! harness both call it, so tests exercise exactly what the window shows.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::Clock;
use easyplay_core::config::Preferences;
use easyplay_core::ids::{GameId, QueryId};
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{PromptAnswer, SaveDataLocation, Tool, WorkingDir};
use egui::{Context, Ui};

use crate::bridge::{
    Command, MAX_EVENTS_PER_FRAME, OpenOrigin, QueryPayload, ToastLevel, UiBridge, UiCommand,
    UiEvent,
};
use crate::input::dnd::DndState;
use crate::input::gamepad::{self, NavIntent, PadGate, PadInput, PadNav};
use crate::input::picker::{NoPicker, PickPurpose, PickResult, Picker};
use crate::route::{Navigator, Overlay, Route};
use crate::screens::gallery::GalleryState;
use crate::screens::shell::{Banner, BannerQueue};
use crate::screens::{Env, shell};
use crate::strings;
use crate::textures::{ArtSource, EguiUploader, Limits, TextureCache};
use crate::theme::tokens::timing;
use crate::theme::{self, Appearance};
use crate::vm::details::DetailsVm;
use crate::vm::dialogs::{AfterDiscard, Confirm, Dialog, DialogQueue};
use crate::vm::launch::LaunchVm;
use crate::vm::library::LibraryVm;
use crate::vm::screens::{AddGameVm, ArtworkVm, OnboardingVm, QuickLaunchVm, SettingsVm, StatsVm};
use crate::vm::setup::{SetupPhase, SetupVm};
use crate::vm::toasts::{ToastAction, ToastKind, ToastQueue};
use crate::vm::{AsyncValue, details};
use crate::widgets::banner::Severity;

/// The UI-thread guard of PERFORMANCE §2 lives in core (`easyplay_core::ui_thread`) so the
/// real `Fs`, `Store`, `CommandRunner` and spawners can check it; [`EasyPlayApp::logic`] and
/// [`EasyPlayApp::paint`] mark the thread for their duration.
pub use easyplay_core::ui_thread::is_ui_thread;

/// Window geometry kept in `<state>/window.json` (PERSISTENCE §1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowGeometry {
    /// Inner size in logical points.
    pub size: [f32; 2],
    /// Outer position, when the platform reports it (not on Wayland).
    pub position: Option<[f32; 2]>,
    /// Maximised.
    pub maximized: bool,
    /// UI scale at exit.
    pub ui_scale: f32,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            size: theme::tokens::layout::DEFAULT_WINDOW,
            position: None,
            maximized: false,
            ui_scale: 1.0,
        }
    }
}

/// Persists window geometry. Implemented by the binary; must not block the caller (hand the
/// value to a writer thread).
pub trait WindowStore {
    /// Save (debounced by the caller; also called at exit).
    fn save(&self, geometry: &WindowGeometry);
}

/// Everything not domain state that the screens keep between frames.
pub struct UiState {
    /// Routes and overlays.
    pub nav: Navigator,
    /// Library screen state.
    pub library: LibraryVm,
    /// Toasts.
    pub toasts: ToastQueue,
    /// Undo commands offered by toasts, by action key.
    pub undo: HashMap<u64, Vec<UiCommand>>,
    /// The last undo key offered (for `Ctrl+Z`).
    pub last_undo: Option<u64>,
    next_undo: u64,
    /// Banner slot.
    pub banners: BannerQueue,
    /// Widget gallery state.
    pub gallery: GalleryState,
    /// A card that should take keyboard focus when next drawn.
    pub pending_focus: Option<GameId>,
    /// Focus the search field next frame (type-to-search, `Ctrl+F`).
    pub focus_search: bool,
    /// Text typed while nothing had focus (type-to-search).
    pub typed: String,
    /// The last window title sent.
    pub last_title: Option<String>,
    /// Game page.
    pub details: DetailsVm,
    /// Launches.
    pub launches: LaunchVm,
    /// Dialogs.
    pub dialogs: DialogQueue,
    /// Setup sheet.
    pub setup: Option<SetupVm>,
    /// Add-game sheet.
    pub add_game: Option<AddGameVm>,
    /// Artwork sheet.
    pub artwork: Option<ArtworkVm>,
    /// Statistics.
    pub stats: StatsVm,
    /// The Chat page (ADR-0033).
    pub chat: crate::vm::chat::ChatVm,
    /// Settings.
    pub settings: SettingsVm,
    /// Onboarding.
    pub onboarding: OnboardingVm,
    /// Quick-launch window.
    pub quick: Option<QuickLaunchVm>,
    /// Onboarding has a compact window of its own, which closes when it finishes.
    pub onboarding_window: bool,
    /// Drag and drop.
    pub dnd: DndState,
    /// Pickers asked for this frame (opened by the app after painting).
    pub picks: Vec<PickPurpose>,
    /// Tag editor text.
    pub tag_text: String,
    /// Folders picked for sharing, applied (and checked) by the screen next frame.
    pub shares: Vec<(Option<GameId>, PathBuf)>,
    /// The newest toast already announced.
    pub announced_toast: Option<crate::vm::toasts::ToastId>,
    /// Controller mode (USER_EXPERIENCE §5.3): larger focus ring, Play always visible,
    /// button hints in the footer.
    pub controller: bool,
    /// Controller intents that are application actions (favourite, menu, search, LB/RB),
    /// handled by the shell this frame.
    pub pad_actions: Vec<NavIntent>,
    /// Controller mode was just entered: put focus on the cursor's card if nothing has it.
    pub pad_entered: bool,
    /// The route shown last frame (focus returns to the opened card after a game page).
    pub prev_route: Option<Route>,
    /// A game reached *Running* since the last frame (the controller pauses).
    pub game_started: bool,
    /// Developer Mode views, the report preview, recovery choices.
    pub debug: crate::vm::debug::DebugVm,
    /// Where covers are, for the flight between the wall and a game page.
    pub flight: crate::screens::flight::Flight,
    /// The page shown in the last pass (a change of page fades).
    pub shown_page: Option<Route>,
    /// A change of page in progress.
    pub turn: Option<shell::PageTurn>,
    /// The Settings tab last visited: where "Settings" in the top bar leads.
    pub settings_tab: crate::route::SettingsTab,
}

impl UiState {
    /// Fresh state starting at `route`.
    pub fn new(route: Route) -> Self {
        Self {
            nav: Navigator::new(route),
            library: LibraryVm::default(),
            toasts: ToastQueue::new(),
            undo: HashMap::new(),
            last_undo: None,
            next_undo: 1,
            banners: BannerQueue::default(),
            gallery: GalleryState::default(),
            pending_focus: None,
            focus_search: false,
            typed: String::new(),
            last_title: None,
            details: DetailsVm::default(),
            launches: LaunchVm::default(),
            dialogs: DialogQueue::default(),
            setup: None,
            add_game: None,
            artwork: None,
            stats: StatsVm::default(),
            chat: Default::default(),
            settings: SettingsVm::default(),
            onboarding: OnboardingVm::default(),
            quick: None,
            onboarding_window: false,
            dnd: DndState::default(),
            picks: Vec::new(),
            tag_text: String::new(),
            shares: Vec::new(),
            announced_toast: None,
            controller: false,
            pad_actions: Vec::new(),
            pad_entered: false,
            prev_route: None,
            game_started: false,
            debug: crate::vm::debug::DebugVm::default(),
            flight: crate::screens::flight::Flight::default(),
            shown_page: None,
            turn: None,
            settings_tab: crate::route::SettingsTab::default(),
        }
    }

    /// Queue a dialog and show it when nothing else is.
    pub fn show_dialog(&mut self, d: Dialog) {
        self.dialogs.push(d);
        self.sync_dialog();
    }

    /// Make the overlay stack show the queue's front dialog.
    pub fn sync_dialog(&mut self) {
        let showing = self
            .nav
            .top_overlay()
            .is_some_and(|o| matches!(o, Overlay::Dialog(_)));
        if !showing && let Some((id, _)) = self.dialogs.front() {
            self.nav.open(Overlay::Dialog(id));
        }
    }

    /// A toast; `undo` adds an *Undo* action that sends the command.
    pub fn toast(
        &mut self,
        text: impl Into<String>,
        kind: ToastKind,
        undo: Option<UiCommand>,
        now: easyplay_core::clock::MonoInstant,
    ) {
        self.toast_undoing(text, kind, undo.into_iter().collect(), now);
    }

    /// A toast whose *Undo* takes several commands (hiding several games); none, and the
    /// toast has no action.
    pub fn toast_undoing(
        &mut self,
        text: impl Into<String>,
        kind: ToastKind,
        undo: Vec<UiCommand>,
        now: easyplay_core::clock::MonoInstant,
    ) {
        let action = (!undo.is_empty()).then(|| {
            let key = self.next_undo;
            self.next_undo += 1;
            self.undo.insert(key, undo);
            self.last_undo = Some(key);
            ToastAction {
                label: strings::feedback::UNDO().to_owned(),
                key,
            }
        });
        self.toasts.push(text, kind, action, now);
    }

    /// Navigate, asking first when the game page has unsaved changes.
    pub fn navigate(&mut self, after: AfterDiscard) {
        let leaving_page =
            matches!(self.nav.current(), Route::Game(_)) && !matches!(after, AfterDiscard::Tab(_));
        if leaving_page && self.details.dirty() {
            self.show_dialog(Dialog::Confirm(Confirm::DiscardChanges(after)));
            return;
        }
        self.apply_nav(after);
    }

    /// Navigate without asking.
    pub fn apply_nav(&mut self, after: AfterDiscard) {
        match after {
            AfterDiscard::Back => {
                self.nav.back();
            }
            AfterDiscard::Route(r) => match r {
                Route::Game(_) => self.nav.push(r),
                other => self.nav.go_to(other),
            },
            AfterDiscard::Tab(i) => {
                self.details.tab =
                    details::DetailsTab::ALL[i.min(details::DetailsTab::ALL.len() - 1)];
            }
        }
    }
}

/// Creates the artwork source once the context exists (so it can wake the UI).
pub type ArtFactory = Box<dyn FnOnce(&Context) -> Box<dyn ArtSource>>;

/// Creates the picker once the context exists.
pub type PickerFactory = Box<dyn FnOnce(&Context) -> Box<dyn Picker>>;

/// The application.
pub struct EasyPlayApp {
    /// Screen state (public for tests).
    pub state: UiState,
    bridge: UiBridge,
    clock: Arc<dyn Clock>,
    utc_offset_s: i32,
    appearance: Appearance,
    textures: Option<TextureCache<EguiUploader>>,
    art: Option<Box<dyn ArtSource>>,
    art_factory: Option<ArtFactory>,
    picker: Option<Box<dyn Picker>>,
    picker_factory: Option<PickerFactory>,
    window_store: Option<Box<dyn WindowStore>>,
    geometry: WindowGeometry,
    geometry_dirty_since: Option<f64>,
    installed: bool,
    pad_rx: Option<std::sync::mpsc::Receiver<PadInput>>,
    pad_queue: Vec<PadInput>,
    pad: PadNav,
    pad_gate: PadGate,
    system: theme::SystemPrefs,
    system_rx: Option<std::sync::mpsc::Receiver<theme::SystemPrefs>>,
}

impl EasyPlayApp {
    /// Build the app. Nothing touches egui until the first frame (or [`Self::install`]).
    pub fn new(
        bridge: UiBridge,
        clock: Arc<dyn Clock>,
        appearance: Appearance,
        start: Route,
        utc_offset_s: i32,
    ) -> Self {
        // First launch: onboarding before the library (SCREEN_SPECS §1).
        let start = if start == Route::Library && !bridge.prefs().onboarding.completed {
            Route::Onboarding
        } else {
            start
        };
        let mut state = UiState::new(start);
        if bridge.is_preview() {
            crate::demo_chat::seed_vm(&mut state.chat);
            state.banners.set(Banner {
                key: "preview",
                severity: Severity::Info,
                text: strings::feedback::PREVIEW_BANNER().to_owned(),
                action: None,
                dismissible: true,
            });
        }
        Self {
            state,
            bridge,
            clock,
            utc_offset_s,
            appearance,
            textures: None,
            art: None,
            art_factory: None,
            picker: None,
            picker_factory: None,
            window_store: None,
            geometry: WindowGeometry::default(),
            geometry_dirty_since: None,
            installed: false,
            pad_rx: None,
            pad_queue: Vec::new(),
            pad: PadNav::default(),
            pad_gate: PadGate::default(),
            system: theme::SystemPrefs::default(),
            system_rx: None,
        }
    }

    /// Use `factory` to create the artwork source (default: none, placeholders only).
    pub fn with_art(mut self, factory: ArtFactory) -> Self {
        self.art_factory = Some(factory);
        self
    }

    /// Use `picker` for file dialogs (default: none; `run` installs the system dialog).
    pub fn with_picker(mut self, picker: Box<dyn Picker>) -> Self {
        self.picker = Some(picker);
        self
    }

    /// Create the picker once the context exists.
    pub fn with_picker_factory(mut self, factory: PickerFactory) -> Self {
        self.picker_factory = Some(factory);
        self
    }

    /// Start in the compact quick-launch layout for `path` (SCREEN_SPECS §7).
    pub fn quick_launch(self, path: PathBuf) -> Self {
        self.bridge.send(Command::OpenPaths {
            paths: vec![path.clone()],
            origin: OpenOrigin::FileManager,
        });
        self.follow_quick_launch(path)
    }

    /// Start in the compact quick-launch layout for `path`, which the resident service
    /// already sent to the service before this window existed.
    pub fn follow_quick_launch(mut self, path: PathBuf) -> Self {
        self.state.quick = Some(QuickLaunchVm {
            path,
            game: None,
            running_at: None,
            already_at: None,
            expanded: false,
            grown: false,
        });
        self
    }

    /// The window is closing while the service may run on (the resident service): a setup
    /// sheet still being edited is discarded and unanswered prompts are cancelled, so their
    /// flows do not stay parked and the next program opened gets its own sheet.
    pub fn abandon_open_flows(&mut self) {
        if self
            .state
            .setup
            .as_ref()
            .is_some_and(|v| v.phase == SetupPhase::Editing)
        {
            self.bridge.send(Command::DiscardDraft);
            self.state.setup = None;
        }
        let prompts: Vec<_> = self.state.dialogs.prompts().map(|p| p.id).collect();
        for prompt in prompts {
            self.bridge.send(Command::AnswerPrompt {
                prompt,
                answer: PromptAnswer::Cancel,
            });
        }
    }

    /// Follow the desktop's appearance preferences: `initial` now, updates from `rx`.
    pub fn with_system_prefs(
        mut self,
        initial: theme::SystemPrefs,
        rx: Option<std::sync::mpsc::Receiver<theme::SystemPrefs>>,
    ) -> Self {
        self.system = initial;
        self.system_rx = rx;
        self
    }

    /// The desktop's preferences changed (tests call this directly).
    pub fn set_system_prefs(&mut self, ctx: &Context, system: theme::SystemPrefs) {
        self.system = system.clone();
        theme::set_system(ctx, system);
        self.reapply_appearance(ctx, true);
    }

    /// Make the interface's language the one `general.language` and the desktop's locale
    /// resolve to; repaint when it changed.
    fn apply_language(&self, ctx: &Context) {
        let choice = crate::i18n::LanguageChoice::parse(&self.bridge.prefs().general.language);
        let language = crate::i18n::resolve(choice, theme::system(ctx).locale.as_deref());
        if language != crate::i18n::language() {
            crate::i18n::set(language);
            ctx.request_repaint();
        }
    }

    /// Recompute the appearance from the preferences and the desktop; restyle if it changed.
    fn reapply_appearance(&mut self, ctx: &Context, zoom_too: bool) {
        self.apply_language(ctx);
        let prefs = self.bridge.prefs().clone();
        let a = appearance_of(&prefs, self.appearance, &self.system);
        if a != self.appearance || zoom_too {
            self.appearance = a;
            theme::apply(ctx, &a);
            ctx.set_zoom_factor(theme::zoom(ctx, &a));
        }
    }

    /// Read controller input from `rx` (the `gilrs` thread of `input::gamepad::spawn`).
    pub fn with_gamepad(mut self, rx: std::sync::mpsc::Receiver<PadInput>) -> Self {
        self.pad_rx = Some(rx);
        self
    }

    /// Queue one controller event as if it came from the controller thread (tests).
    pub fn pad_input(&mut self, ev: PadInput) {
        self.pad_queue.push(ev);
    }

    /// Turn pending controller input into egui input for the coming frame
    /// (`raw_input_hook`). Real keyboard or pointer activity leaves controller mode first.
    pub fn pump_gamepad(&mut self, ctx: &Context, raw: &mut egui::RawInput) {
        if gamepad::has_desktop_input(raw) {
            self.state.controller = false;
        }
        let mut inputs = std::mem::take(&mut self.pad_queue);
        if let Some(rx) = &self.pad_rx {
            inputs.extend(rx.try_iter());
        }
        let now = raw.time.unwrap_or_else(|| ctx.input(|i| i.time));
        let enabled = self.bridge.prefs().integration.gamepad_navigation;
        let started = std::mem::take(&mut self.state.game_started);
        if !self.pad_gate.update(enabled, raw.focused, started) {
            // While a game runs (or the window is in the background) nothing is read.
            self.pad = PadNav::default();
            return;
        }
        let mut intents = Vec::new();
        for ev in inputs {
            intents.extend(self.pad.input(ev, now));
        }
        let (repeats, next) = self.pad.tick(now);
        intents.extend(repeats);
        if let Some(n) = next {
            ctx.request_repaint_after(Duration::from_secs_f64(n.max(0.001)));
        }
        if intents.is_empty() {
            return;
        }
        if !self.state.controller {
            // The first press only enters controller mode and shows where focus is (UF-15
            // step 2); acting on it could move focus from a place the user cannot see yet.
            self.state.controller = true;
            self.state.pad_entered = true;
            ctx.request_repaint();
            return;
        }
        let focus = match ctx.memory(|m| m.focused()) {
            None => gamepad::PadFocus::Nothing,
            Some(f) if crate::screens::library::is_game_focus(&self.state, f) => {
                gamepad::PadFocus::Game
            }
            Some(_) => gamepad::PadFocus::Control,
        };
        for intent in intents {
            match gamepad::key_events(intent, focus) {
                Some(events) => {
                    if matches!(intent, NavIntent::Scroll(_))
                        && ctx.input(|i| i.pointer.latest_pos()).is_none()
                    {
                        raw.events
                            .push(egui::Event::PointerMoved(ctx.content_rect().center()));
                    }
                    raw.events.extend(events);
                }
                None => self.state.pad_actions.push(intent),
            }
        }
        ctx.request_repaint();
    }

    /// Save geometry through `store`, starting from `initial`.
    pub fn with_window_store(
        mut self,
        store: Box<dyn WindowStore>,
        initial: WindowGeometry,
    ) -> Self {
        self.window_store = Some(store);
        self.geometry = initial;
        self
    }

    /// Install fonts and theme, waker and texture cache on `ctx`. Called before the first
    /// frame by `run`; [`Self::frame`] calls it lazily otherwise.
    pub fn install(&mut self, ctx: &Context) {
        if self.installed {
            return;
        }
        let _ui = easyplay_core::ui_thread::enter();
        theme::set_system(ctx, self.system.clone());
        if self.system != theme::SystemPrefs::default() {
            let prefs = self.bridge.prefs().clone();
            self.appearance = appearance_of(&prefs, self.appearance, &self.system);
        }
        theme::install(ctx, &self.appearance);
        self.apply_language(ctx);
        ctx.set_zoom_factor(theme::zoom(ctx, &self.appearance));
        let waker_ctx = ctx.clone();
        self.bridge
            .set_waker(Arc::new(move || waker_ctx.request_repaint()));
        self.textures = Some(TextureCache::new(
            EguiUploader(ctx.clone()),
            Limits::default(),
        ));
        self.art = Some(match self.art_factory.take() {
            Some(f) => f(ctx),
            None => Box::new(NoArt),
        });
        if self.picker.is_none() {
            self.picker = Some(match self.picker_factory.take() {
                Some(f) => f(ctx),
                None => Box::new(NoPicker),
            });
        }
        ctx.options_mut(|o| {
            o.zoom_with_keyboard = true;
            // One wheel notch moves the page as far as a browser does (egui's 40 felt slow).
            o.input_options.line_scroll_speed = theme::apply::LINE_SCROLL;
        });
        self.installed = true;
    }

    /// The bridge (tests).
    pub fn bridge(&self) -> &UiBridge {
        &self.bridge
    }

    /// Per-frame logic: drain events, route them, collect picker results, tick toasts.
    /// Never blocks.
    pub fn logic(&mut self, ctx: &Context) {
        let _ui = easyplay_core::ui_thread::enter();
        self.install(ctx);
        let mono = self.clock.mono();
        let latest = self.system_rx.as_ref().and_then(|rx| rx.try_iter().last());
        if let Some(sp) = latest
            && sp != self.system
        {
            self.set_system_prefs(ctx, sp);
        }
        let events = self.bridge.drain();
        if events.len() >= MAX_EVENTS_PER_FRAME {
            // The rest waits in the backend, and nothing else would wake the UI for it.
            ctx.request_repaint();
        }
        for ev in events {
            self.on_event(ctx, ev);
        }
        while let Some(r) = self.picker.as_ref().and_then(|p| p.try_result()) {
            self.on_pick(ctx, r);
        }
        if let Some(next) = self.state.toasts.tick(mono) {
            ctx.request_repaint_after(next.max(Duration::from_millis(16)));
        }
        self.track_geometry(ctx);
    }

    fn on_event(&mut self, ctx: &Context, ev: UiEvent) {
        let mono = self.clock.mono();
        let s = &mut self.state;
        match ev {
            UiEvent::Toast(t) => {
                let kind = match t.level {
                    ToastLevel::Info => ToastKind::Info,
                    ToastLevel::Success => ToastKind::Success,
                    ToastLevel::Warning => ToastKind::Warning,
                };
                s.toast(t.text, kind, t.undo.map(|b| *b), mono);
            }
            UiEvent::Failure(e) => {
                // A failed artwork request arrives as a plain failure: a Steam download that
                // failed is said under its results; anything else stops the spinners.
                if let Some(a) = &mut s.artwork {
                    if a.fetching.is_some() && e.code.area() == "ART" {
                        a.settled();
                        a.fetch_error = Some(e.message);
                        return;
                    }
                    a.settled();
                }
                // Chat: the relays out of reach, or a link that is not one, are passing
                // states, said in one line; the engine retries and the page shows "offline".
                if matches!(
                    e.code,
                    easyplay_core::error::ErrorCode::ChtNoConnection
                        | easyplay_core::error::ErrorCode::ChtBadLink
                ) {
                    s.toast(e.title, ToastKind::Warning, None, mono);
                    return;
                }
                s.show_dialog(Dialog::Error(e));
            }
            UiEvent::Prompt(p) => s.show_dialog(Dialog::Prompt(p)),
            UiEvent::LaunchProgress { game, state } => {
                // The followed program is the game's main one, or another program in its
                // folder (the snapshot lists only the main one).
                if let Some(q) = &mut s.quick
                    && q.game.is_none()
                    && self.bridge.library().get(game).is_some_and(|g| {
                        *g.primary_exe_path == *q.path
                            || g.root.as_deref().is_some_and(|r| q.path.starts_with(r))
                    })
                {
                    q.game = Some(game);
                }
                // Running, or already over without a failure (a program that ends at once
                // never shows as running): the window has done its part and closes.
                if let Some(q) = &mut s.quick
                    && q.game == Some(game)
                    && matches!(
                        state,
                        LaunchState::Running { .. } | LaunchState::Finished { .. }
                    )
                {
                    q.running_at = Some(mono);
                    ctx.request_repaint_after(Duration::from_millis(600));
                }
                if let Some(q) = &mut s.quick
                    && q.game == Some(game)
                    && q.already_at.is_none()
                    && matches!(&state, LaunchState::Failed(u)
                        if u.code == easyplay_core::error::ErrorCode::SesAlreadyRunning)
                {
                    q.already_at = Some(mono);
                    ctx.request_repaint_after(crate::screens::quick_launch::ALREADY_CLOSE);
                }
                if let Some(setup) = &mut s.setup
                    && setup.phase == (SetupPhase::Launching { game: None })
                    && self
                        .bridge
                        .library()
                        .get(game)
                        .is_some_and(|g| *g.primary_exe_path == *setup.draft.path)
                {
                    setup.phase = SetupPhase::Launching { game: Some(game) };
                }
                let name = self
                    .bridge
                    .library()
                    .get(game)
                    .map(|g| g.name.to_string())
                    .unwrap_or_default();
                announce_launch(ctx, &name, s.launches.games.get(&game), &state, mono);
                if matches!(state, LaunchState::Running { .. })
                    && !s
                        .launches
                        .games
                        .get(&game)
                        .is_some_and(|p| matches!(p.state, LaunchState::Running { .. }))
                {
                    s.game_started = true;
                }
                s.launches.update(game, state, mono);
            }
            UiEvent::SetupRequested { draft, context } => {
                let a = self
                    .bridge
                    .assess(None, &draft.name, draft.root.as_deref(), &draft.policy);
                s.setup = Some(SetupVm::new(*draft, context, a));
                s.add_game = None;
                s.nav.open(Overlay::Setup);
            }
            UiEvent::QueryResult { id, payload } => self.on_query(id, payload),
            UiEvent::ArtworkFailed { game, slot, error } => {
                if let Some(a) = &mut s.artwork
                    && a.game == game
                {
                    a.busy.retain(|x| *x != slot);
                    a.fetching = None;
                    a.errors.retain(|(x, _)| *x != slot);
                    a.errors.push((slot, error.message.clone()));
                } else {
                    s.toast(error.title, ToastKind::Warning, None, mono);
                }
            }
            UiEvent::LibraryChanged => {
                let lib = self.bridge.library().clone();
                if let Some(a) = &mut s.artwork {
                    a.settled();
                    a.errors.clear();
                }
                if let Route::Game(id) = s.nav.current()
                    && lib.get(id).is_none()
                    && id.is_steam()
                {
                    // A Steam game leaves when Steam no longer lists it or the option is
                    // turned off (ADR-0031): its page closes; nothing was removed here.
                    s.nav.back();
                } else if let Route::Game(id) = s.nav.current()
                    && lib.get(id).is_none()
                {
                    let name = s
                        .details
                        .record()
                        .map(|d| d.game.name.clone())
                        .unwrap_or_default();
                    s.nav.back();
                    s.toast(
                        strings::feedback::removed(&name),
                        ToastKind::Info,
                        None,
                        mono,
                    );
                }
                s.library.selection.retain(|g| lib.get(*g).is_some());
            }
            UiEvent::PreferencesChanged => {
                self.reapply_appearance(ctx, false);
            }
            UiEvent::HostChanged | UiEvent::RuntimesChanged | UiEvent::DiagnosticsChanged => {}
            // The Chat page reads the snapshot each frame; its conversation view-model
            // takes the messages (ADR-0033).
            UiEvent::ChatChanged => {}
            UiEvent::ChatMessages { messages, updated } => {
                s.chat.on_messages(&messages, updated);
            }
            // The link check flagged a message's link or got no answer: ask (services).
            UiEvent::ChatUnsafeLink { url, unchecked, .. } => {
                s.chat.link_to_open = Some(url);
                s.chat.link_unchecked = unchecked;
                s.show_dialog(Dialog::Chat(crate::vm::chat::ChatDialog::UnsafeLink));
            }
            UiEvent::ShowGame(g) => {
                if let Some(q) = &mut s.quick {
                    q.game = Some(g);
                } else {
                    s.navigate(AfterDiscard::Route(Route::Game(g)));
                }
            }
            UiEvent::QuitRequested => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn on_query(&mut self, id: QueryId, payload: QueryPayload) {
        let Some(payload) = crate::screens::debug::on_query(&mut self.state, id, payload) else {
            return;
        };
        let s = &mut self.state;
        if s.chat.on_query(id, &payload) || s.chat.gif.on_query(id, &payload) {
            return;
        }
        if let Some(a) = &mut s.artwork
            && a.apps_q == Some(id)
        {
            a.apps_q = None;
            a.apps = match payload {
                QueryPayload::SteamApps {
                    detected,
                    candidates,
                } => AsyncValue::Ready(crate::vm::screens::SteamApps {
                    detected,
                    candidates,
                }),
                QueryPayload::Failed(e) => AsyncValue::Failed(e.message),
                _ => AsyncValue::Failed(String::new()),
            };
            return;
        }
        let d = &mut s.details;
        if d.steam_q == Some(id) {
            d.steam_q = None;
            d.steam = match payload {
                QueryPayload::Steam(x) => {
                    // The notes arrive with it; typing in them meanwhile is not overwritten.
                    if d.notes_edited.is_none() {
                        d.notes.clone_from(&x.notes);
                    }
                    AsyncValue::Ready(x)
                }
                QueryPayload::Failed(e) => AsyncValue::Failed(e),
                _ => AsyncValue::Idle,
            };
            return;
        }
        if d.detail_q == Some(id) {
            d.detail_q = None;
            match payload {
                QueryPayload::Game(g) => {
                    let a = self.bridge.assess(
                        Some(g.game.id),
                        &g.game.name,
                        g.game.root.as_deref(),
                        &g.game.policy,
                    );
                    let rev = self.bridge.library().revision;
                    d.loaded(g, a, rev);
                }
                QueryPayload::Failed(e) => d.detail = AsyncValue::Failed(e),
                _ => {}
            }
            return;
        }
        if d.about_q == Some(id) {
            d.about_q = None;
            d.about = match payload {
                QueryPayload::About(a) => AsyncValue::Ready(a),
                QueryPayload::Failed(e) => AsyncValue::Failed(e),
                _ => AsyncValue::Idle,
            };
            return;
        }
        if d.activity.overview_q == Some(id) {
            d.activity.overview_q = None;
            d.activity.overview = match payload {
                QueryPayload::Stats(easyplay_core::stats::StatsResult::Game(o)) => {
                    AsyncValue::Ready(o)
                }
                QueryPayload::Failed(e) => AsyncValue::Failed(e),
                _ => AsyncValue::Idle,
            };
            return;
        }
        if d.week_q == Some(id) {
            d.week_q = None;
            match payload {
                QueryPayload::Stats(easyplay_core::stats::StatsResult::Game(o)) => {
                    d.week = AsyncValue::Ready(o);
                }
                // A failed refresh keeps the week that is on screen.
                QueryPayload::Failed(e) if d.week.ready().is_none() => {
                    d.week = AsyncValue::Failed(e);
                }
                _ => {}
            }
            return;
        }
        if d.activity.history_q == Some(id) {
            d.activity.history_q = None;
            if let QueryPayload::Stats(easyplay_core::stats::StatsResult::History(h)) = payload {
                d.activity.history.extend(h.rows);
                d.activity.has_more = h.has_more;
                d.activity.next_page += 1;
            } else {
                // Not asked again every frame: *Load more* tries again.
                d.activity.history_failed = true;
                d.activity.has_more = true;
            }
            return;
        }
        if d.size_q == Some(id) {
            d.size_q = None;
            match payload {
                QueryPayload::PrefixSize(n) => d.prefix_size = AsyncValue::Ready(n),
                QueryPayload::Failed(e) => d.prefix_size = AsyncValue::Failed(e.message),
                _ => {}
            }
            return;
        }
        if d.fixes_q == Some(id) {
            d.fixes_q = None;
            // An answer either way: an unanswered search would be sent again every frame.
            d.fixes = match payload {
                QueryPayload::Fixes(f) => AsyncValue::Ready(f),
                QueryPayload::Failed(e) => AsyncValue::Failed(e.message),
                _ => AsyncValue::Ready(Vec::new()),
            };
            return;
        }
        let st = &mut s.stats;
        if let Some((q, range, rev, _)) = st.pending
            && q == id
        {
            st.pending = None;
            match payload {
                QueryPayload::Stats(easyplay_core::stats::StatsResult::Library(o)) => {
                    st.failed = None;
                    st.cache.insert((range, rev), o);
                }
                QueryPayload::Failed(e) => st.failed = Some(e),
                _ => {}
            }
            return;
        }
        if st.recent_q.is_some_and(|(q, _)| q == id) {
            st.recent = match payload {
                QueryPayload::Stats(easyplay_core::stats::StatsResult::History(h)) => {
                    AsyncValue::Ready(h.rows)
                }
                QueryPayload::Failed(e) => AsyncValue::Failed(e),
                _ => AsyncValue::Idle,
            };
            return;
        }
        if let Some((q, range, rev)) = st.top_q
            && q == id
        {
            st.top_q = None;
            if let QueryPayload::Stats(easyplay_core::stats::StatsResult::Top(t)) = payload {
                st.top.insert((range, rev), t);
            }
            return;
        }
        if st.weekdays_q == Some(id) {
            st.weekdays_q = None;
            if let QueryPayload::Stats(easyplay_core::stats::StatsResult::Weekdays(w)) = payload {
                st.weekdays = Some(w);
            }
            return;
        }
        if st.history_q == Some(id) {
            st.history_q = None;
            if let QueryPayload::Stats(easyplay_core::stats::StatsResult::History(h)) = payload {
                st.history.extend(h.rows);
                st.history_more = h.has_more;
                st.history_page += 1;
            } else {
                // The sheet asks again when it is opened next, not every frame.
                st.history_more = false;
            }
            return;
        }
        if let QueryPayload::Folder(scan) = payload {
            let mine = s.add_game.as_ref().is_some_and(|a| a.query == Some(id));
            if mine || id.as_uuid().is_nil() {
                let a = s.add_game.get_or_insert_with(AddGameVm::default);
                a.folder.clone_from(&scan.folder);
                a.set_scan(scan);
                if s.nav.top_overlay() != Some(Overlay::AddGame) {
                    s.nav.open(Overlay::AddGame);
                }
            }
        }
    }

    fn on_pick(&mut self, ctx: &Context, r: PickResult) {
        let s = &mut self.state;
        let Some(first) = r.paths.first().cloned() else {
            return;
        };
        match r.purpose {
            PickPurpose::SaveReport => {
                crate::screens::debug::report::save_to(s, &self.bridge, first);
            }
            // The dialog stays open between its steps.
            PickPurpose::AddGameFile => {
                crate::screens::add_game::picked_programs(ctx, s, &self.bridge, r.paths);
            }
            PickPurpose::AddGameFolder => crate::screens::add_game::scan(s, &self.bridge, first),
            PickPurpose::FolderGrant(game) => s.shares.push((game, first)),
            PickPurpose::RelocateFolder(game, old) => {
                crate::screens::details::permissions::relocate_folder(s, game, &old, first);
            }
            PickPurpose::SaveFolder(game) => {
                let loc = SaveDataLocation::External { dir: first };
                match game {
                    Some(g) if s.details.game == Some(g) => {
                        if let Some(p) = &mut s.details.protection {
                            p.draft.save_data = loc;
                            p.reclassify();
                            let draft = p.draft.clone();
                            let a = assess_draft(&self.bridge, &s.details, &draft);
                            if let Some(p) = &mut s.details.protection {
                                p.assessment = a;
                            }
                        }
                    }
                    _ => {
                        if let Some(setup) = &mut s.setup {
                            setup.draft.policy.save_data = loc;
                            setup.reclassify();
                        }
                    }
                }
            }
            PickPurpose::SetupRoot => {
                if let Some(setup) = &mut s.setup {
                    setup.set_root(Some(first));
                    setup.assessment = self.bridge.assess(
                        None,
                        &setup.name,
                        setup.draft.root.as_deref(),
                        &setup.draft.policy,
                    );
                }
            }
            PickPurpose::Artwork(game, slot) => {
                if let Some(a) = &mut s.artwork {
                    a.busy.push(slot);
                    a.errors.retain(|(x, _)| *x != slot);
                }
                self.bridge.send(UiCommand::ImportArtwork {
                    id: game,
                    slot,
                    source: crate::bridge::ArtworkSource::File(first),
                });
            }
            PickPurpose::AddProgram(game) => self.bridge.send(Command::AddExecutable {
                id: game,
                path: first,
                role: easyplay_core::model::ExeRole::Alternate,
            }),
            // In a Steam game's Windows storage (ADR-0031).
            PickPurpose::RunProgram(game) if game.is_steam() => {
                self.bridge
                    .send(UiCommand::Steam(crate::bridge::SteamAction::RunProgram {
                        id: game,
                        program: first,
                        args: Vec::new(),
                    }));
            }
            PickPurpose::RunProgram(game) => self.bridge.send(Command::RunTool {
                id: game,
                tool: Tool::RunExe {
                    path: first,
                    args: Vec::new(),
                },
            }),
            PickPurpose::Locate(game) => self.bridge.send(Command::LocateExecutable {
                id: game,
                exe: None,
                new_path: first,
            }),
            PickPurpose::WorkingDir(game) => {
                if s.details.game == Some(game)
                    && let Some(l) = &mut s.details.launch
                {
                    l.config.working_dir = WorkingDir::Custom(first);
                }
            }
            PickPurpose::LayerFolder => {
                let prefs = self.bridge.prefs().clone();
                let mut next = (*prefs).clone();
                next.runtime.extra_search_dirs.push(first);
                let list: Vec<String> = next
                    .runtime
                    .extra_search_dirs
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                let patch = easyplay_core::config::PreferencePatch::new()
                    .set("runtime.extra_search_dirs", list);
                self.bridge.send(UiCommand::SetPreference {
                    patch,
                    preview: Box::new(next.clone()),
                });
                self.bridge.set_prefs_preview(next);
            }
            // The Chat composer's + menu (composer agent): the chosen files join the
            // conversation's attachments (sent with the next message, the text as the first
            // one's caption); for a conversation no longer open they go at once.
            // *Save as…* of a Chat message's file (conversation agent): the service copies it.
            PickPurpose::ChatSave { chat, item, .. } => {
                self.bridge.send(Command::Chat(
                    easyplay_core::service::ChatCommand::SaveFile {
                        chat,
                        item,
                        to: first,
                    },
                ));
            }
            PickPurpose::ChatPicture(chat) | PickPurpose::ChatFile(chat) => {
                if s.chat.open.as_ref().is_some_and(|o| o.chat == chat) {
                    let files = r
                        .paths
                        .into_iter()
                        .map(crate::vm::chat::Attachment::File)
                        .collect();
                    s.chat.attach(files);
                    s.chat.focus_composer = true;
                } else {
                    self.bridge.send(Command::Chat(
                        easyplay_core::service::ChatCommand::SendFile {
                            chat,
                            path: first,
                            text: String::new(),
                        },
                    ));
                }
            }
        }
    }

    /// Logic plus painting: one complete frame (the kittest entry point).
    pub fn frame(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.logic(&ctx);
        self.paint(ui);
    }

    /// Paint the UI for this frame (eframe calls `logic` separately).
    pub fn paint(&mut self, ui: &mut Ui) {
        let _ui = easyplay_core::ui_thread::enter();
        let ctx = ui.ctx().clone();
        self.install(&ctx);
        // A language chosen in Settings applies at once, before the preference round trip.
        self.apply_language(&ctx);
        // Emoji are glyphs of their own: installed before any page lays out text.
        crate::screens::chat::prepare(&ctx, self.bridge.prefs());
        let (Some(textures), Some(art)) = (self.textures.as_mut(), self.art.as_deref()) else {
            return;
        };
        textures.begin_frame();
        self.state.dnd.begin_frame();
        let mut env = Env {
            bridge: &mut self.bridge,
            now: self.clock.now(),
            mono: self.clock.mono(),
            utc_offset_s: self.utc_offset_s,
            textures,
            art,
            ctx: ctx.clone(),
        };
        shell::show(ui, &mut self.state, &mut env);
        env.textures.end_frame(art);
        if env.textures.waiting_upload() > 0 {
            ctx.request_repaint();
        }
        if let Some(p) = &self.picker {
            for purpose in self.state.picks.drain(..) {
                p.pick(purpose);
            }
        }
    }

    fn track_geometry(&mut self, ctx: &Context) {
        // The compact quick-launch window's size is not the library's.
        if self.window_store.is_none() || self.state.quick.as_ref().is_some_and(|q| !q.expanded) {
            return;
        }
        let (inner, outer, maximized) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.maximized.unwrap_or(false))
        });
        let mut g = self.geometry;
        if let Some(r) = inner
            && !maximized
        {
            g.size = [r.width(), r.height()];
        }
        g.position = outer.map(|r| [r.min.x, r.min.y]);
        g.maximized = maximized;
        g.ui_scale = ctx.zoom_factor() / theme::system(ctx).text_scale.max(0.1);
        let now = ctx.input(|i| i.time);
        if g != self.geometry {
            self.geometry = g;
            self.geometry_dirty_since = Some(now);
        }
        if let Some(t) = self.geometry_dirty_since {
            let left = f64::from(timing::GEOMETRY_DEBOUNCE) - (now - t);
            if left <= 0.0 {
                self.save_geometry();
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(left));
            }
        }
    }

    fn save_geometry(&mut self) {
        if let Some(s) = &self.window_store {
            s.save(&self.geometry);
        }
        self.geometry_dirty_since = None;
    }
}

/// Assess a Protection draft for the game on the page.
pub fn assess_draft(
    bridge: &UiBridge,
    d: &DetailsVm,
    policy: &easyplay_core::model::SandboxPolicy,
) -> crate::bridge::PolicyAssessment {
    let (name, root) = d
        .record()
        .map(|r| (r.game.name.clone(), r.game.root.clone()))
        .unwrap_or_default();
    bridge.assess(d.game, &name, root.as_deref(), policy)
}

/// The theme appearance of `prefs` (UI scale and density included).
///
/// "Follow the system" resolves against the desktop (`system`): high contrast when the desktop
/// asks for it, reduced motion when it does (A11Y-004).
pub fn appearance_of(
    prefs: &Preferences,
    current: Appearance,
    system: &theme::SystemPrefs,
) -> Appearance {
    use easyplay_core::config::schema::{GridDensity, ReduceMotion};
    let a = &prefs.appearance;
    let choice = theme::ThemeChoice::parse(a.theme.as_str()).unwrap_or(current.theme);
    Appearance {
        theme: if choice == theme::ThemeChoice::System && system.high_contrast {
            theme::ThemeChoice::HighContrast
        } else {
            choice
        },
        accent: theme::Accent::parse(a.accent.as_str()).unwrap_or(current.accent),
        reduce_motion: match a.reduce_motion {
            ReduceMotion::On => true,
            ReduceMotion::Off => false,
            ReduceMotion::System => system.reduced_motion.unwrap_or(current.reduce_motion),
        },
        ui_scale: a.ui_scale as f32,
        density: match a.grid_density {
            GridDensity::Compact => theme::Density::Compact,
            GridDensity::Comfortable => theme::Density::Comfortable,
            GridDensity::Large => theme::Density::Large,
        },
    }
}

/// Launch announcements (ACCESSIBILITY §4.3): phase changes and start/end politely,
/// failures assertively, download progress at most every 15 s.
fn announce_launch(
    ctx: &Context,
    name: &str,
    prev: Option<&crate::vm::launch::GameLaunch>,
    state: &LaunchState,
    now: easyplay_core::clock::MonoInstant,
) {
    use crate::a11y::{Announcer, DOWNLOAD_EVERY_S, Politeness};
    use crate::strings::a11y as say;
    use easyplay_core::launch::StateKind;
    let prev_kind = prev.map(|p| p.state.kind());
    let prev_phase = prev.map(|p| crate::vm::launch::present(&p.state, 0.0).phase);
    match state {
        LaunchState::Running { .. } => {
            if prev_kind != Some(StateKind::Running) {
                Announcer::announce(ctx, say::running(name), Politeness::Polite);
            }
        }
        LaunchState::Failed(e) => {
            Announcer::announce(ctx, e.title.clone(), Politeness::Assertive);
        }
        LaunchState::Finished { .. } => {
            let text = match prev {
                Some(p) if p.state.kind() == StateKind::Running => {
                    let ms = now.saturating_duration_since(p.since).as_millis();
                    say::ended_after(name, u64::try_from(ms).unwrap_or(u64::MAX))
                }
                _ => say::ended(name),
            };
            Announcer::announce(ctx, text, Politeness::Polite);
        }
        LaunchState::Cancelled => {}
        other => {
            let phase = crate::vm::launch::present(other, 0.0).phase;
            if prev_phase.as_deref() != Some(phase.as_str()) {
                Announcer::announce(ctx, say::launch_phase(name, &phase), Politeness::Polite);
            } else if let LaunchState::Provisioning { bytes, .. } = other {
                Announcer::announce_throttled(
                    ctx,
                    "download",
                    say::downloading(*bytes),
                    Politeness::Polite,
                    DOWNLOAD_EVERY_S,
                );
            }
        }
    }
}

struct NoArt;

impl ArtSource for NoArt {
    fn request(&self, _key: crate::textures::TexKey) {}
    fn cancel(&self, _key: crate::textures::TexKey) {}
    fn try_recv(&self) -> Option<crate::textures::Decoded> {
        None
    }
}

impl eframe::App for EasyPlayApp {
    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        self.pump_gamepad(ctx, raw_input);
    }

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        EasyPlayApp::logic(self, ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.paint(ui);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_geometry();
        self.abandon_open_flows();
    }
}
