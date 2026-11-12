//! `EasyPlayApp` implementing `eframe::App`: `logic` (drain events, route them to the
//! view-models, collect picker results), `ui` (shell + current route + overlays),
//! `raw_input_hook` (drops arrive through egui's input), `on_exit` (save geometry). See
//! MODULES §8 and task UI-001.
//!
//! [`EasyPlayApp::frame`] is the whole per-frame UI; the eframe `ui` hook and the kittest
//! harness both call it, so tests exercise exactly what the window shows.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::Clock;
use easyplay_core::config::Preferences;
use easyplay_core::ids::{GameId, QueryId};
use easyplay_core::launch::LaunchState;
use easyplay_core::model::{SaveDataLocation, Tool, WorkingDir};
use egui::{Context, Ui};

use crate::bridge::{OpenOrigin, QueryPayload, ToastLevel, UiBridge, UiCommand, UiEvent};
use crate::input::dnd::DndState;
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

thread_local! {
    static UI_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Mark the current thread as the UI thread (debug guard of PERFORMANCE §2).
pub fn mark_ui_thread() {
    UI_THREAD.with(|f| f.set(true));
}

/// Whether the current thread is the UI thread.
pub fn is_ui_thread() -> bool {
    UI_THREAD.with(Cell::get)
}

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
    pub undo: HashMap<u64, UiCommand>,
    /// The last undo key offered (for `Ctrl+Z`).
    pub last_undo: Option<u64>,
    next_undo: u64,
    /// The user's sidebar choice (`Some(collapsed)`), else automatic.
    pub sidebar_pref: Option<bool>,
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
    /// Settings.
    pub settings: SettingsVm,
    /// Onboarding.
    pub onboarding: OnboardingVm,
    /// Quick-launch window.
    pub quick: Option<QuickLaunchVm>,
    /// Drag and drop.
    pub dnd: DndState,
    /// Pickers asked for this frame (opened by the app after painting).
    pub picks: Vec<PickPurpose>,
    /// Tag editor text.
    pub tag_text: String,
    /// Folders picked for sharing, applied (and checked) by the screen next frame.
    pub shares: Vec<(Option<GameId>, PathBuf)>,
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
            sidebar_pref: None,
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
            settings: SettingsVm::default(),
            onboarding: OnboardingVm::default(),
            quick: None,
            dnd: DndState::default(),
            picks: Vec::new(),
            tag_text: String::new(),
            shares: Vec::new(),
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
        let action = undo.map(|cmd| {
            let key = self.next_undo;
            self.next_undo += 1;
            self.undo.insert(key, cmd);
            self.last_undo = Some(key);
            ToastAction {
                label: strings::feedback::UNDO.to_owned(),
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
                self.details.tab = details::DetailsTab::ALL[i.min(4)];
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
            state.banners.set(Banner {
                key: "preview",
                severity: Severity::Info,
                text: strings::feedback::PREVIEW_BANNER.to_owned(),
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
    pub fn quick_launch(mut self, path: PathBuf) -> Self {
        self.bridge.send(UiCommand::OpenPaths {
            paths: vec![path.clone()],
            origin: OpenOrigin::FileManager,
        });
        self.state.quick = Some(QuickLaunchVm {
            path,
            game: None,
            running_at: None,
            expanded: false,
        });
        self
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
        mark_ui_thread();
        theme::install(ctx, &self.appearance);
        ctx.set_zoom_factor(self.appearance.ui_scale);
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
        ctx.options_mut(|o| o.zoom_with_keyboard = true);
        self.installed = true;
    }

    /// The bridge (tests).
    pub fn bridge(&self) -> &UiBridge {
        &self.bridge
    }

    /// Per-frame logic: drain events, route them, collect picker results, tick toasts.
    /// Never blocks.
    pub fn logic(&mut self, ctx: &Context) {
        self.install(ctx);
        let mono = self.clock.mono();
        for ev in self.bridge.drain() {
            self.on_event(ctx, ev);
        }
        while let Some(r) = self.picker.as_ref().and_then(|p| p.try_result()) {
            self.on_pick(r);
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
            UiEvent::Failure(e) => s.show_dialog(Dialog::Error(e)),
            UiEvent::Prompt(p) => s.show_dialog(Dialog::Prompt(p)),
            UiEvent::LaunchProgress { game, state } => {
                if let Some(q) = &mut s.quick
                    && q.game.is_none()
                    && self
                        .bridge
                        .library()
                        .get(game)
                        .is_some_and(|g| *g.primary_exe_path == *q.path)
                {
                    q.game = Some(game);
                }
                if let Some(q) = &mut s.quick
                    && q.game == Some(game)
                    && matches!(state, LaunchState::Running { .. })
                {
                    q.running_at = Some(mono);
                    ctx.request_repaint_after(Duration::from_millis(600));
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
                    a.errors.retain(|(x, _)| *x != slot);
                    a.errors.push((slot, error.message.clone()));
                } else {
                    s.toast(error.title, ToastKind::Warning, None, mono);
                }
            }
            UiEvent::LibraryChanged => {
                let lib = self.bridge.library().clone();
                if let Some(a) = &mut s.artwork {
                    a.busy.clear();
                    a.errors.clear();
                }
                if let Route::Game(id) = s.nav.current()
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
                let prefs = self.bridge.prefs().clone();
                let a = appearance_of(&prefs, self.appearance);
                if a != self.appearance {
                    self.appearance = a;
                    theme::apply(ctx, &a);
                    ctx.set_zoom_factor(a.ui_scale);
                }
            }
            UiEvent::HostChanged | UiEvent::RuntimesChanged => {}
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
        let s = &mut self.state;
        let d = &mut s.details;
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
        if d.activity.history_q == Some(id) {
            d.activity.history_q = None;
            if let QueryPayload::Stats(easyplay_core::stats::StatsResult::History(h)) = payload {
                d.activity.history.extend(h.rows);
                d.activity.has_more = h.has_more;
                d.activity.next_page += 1;
            }
            return;
        }
        if d.size_q == Some(id) {
            d.size_q = None;
            if let QueryPayload::PrefixSize(n) = payload {
                d.prefix_size = AsyncValue::Ready(n);
            }
            return;
        }
        if d.fixes_q == Some(id) {
            d.fixes_q = None;
            if let QueryPayload::Fixes(f) = payload {
                d.fixes = AsyncValue::Ready(f);
            }
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

    fn on_pick(&mut self, r: PickResult) {
        let s = &mut self.state;
        let Some(first) = r.paths.first().cloned() else {
            return;
        };
        match r.purpose {
            PickPurpose::AddGame | PickPurpose::AddGameFile => {
                s.add_game = None;
                if s.nav.top_overlay() == Some(Overlay::AddGame) {
                    s.nav.close(Overlay::AddGame);
                }
                self.bridge.send(UiCommand::OpenPaths {
                    paths: r.paths,
                    origin: OpenOrigin::Picker,
                });
            }
            PickPurpose::FolderGrant(game) => s.shares.push((game, first)),
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
            PickPurpose::AddProgram(game) => self.bridge.send(UiCommand::AddExecutable {
                id: game,
                path: first,
                role: easyplay_core::model::ExeRole::Alternate,
            }),
            PickPurpose::RunProgram(game) => self.bridge.send(UiCommand::RunTool {
                id: game,
                tool: Tool::RunExe {
                    path: first,
                    args: Vec::new(),
                },
            }),
            PickPurpose::Locate(game) => self.bridge.send(UiCommand::LocateExecutable {
                id: game,
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
        let ctx = ui.ctx().clone();
        self.install(&ctx);
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
        if self.window_store.is_none() {
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
        g.ui_scale = ctx.zoom_factor();
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
pub fn appearance_of(prefs: &Preferences, current: Appearance) -> Appearance {
    use easyplay_core::config::schema::{GridDensity, ReduceMotion};
    let a = &prefs.appearance;
    Appearance {
        theme: theme::ThemeChoice::parse(a.theme.as_str()).unwrap_or(current.theme),
        accent: theme::Accent::parse(a.accent.as_str()).unwrap_or(current.accent),
        reduce_motion: match a.reduce_motion {
            ReduceMotion::On => true,
            ReduceMotion::Off => false,
            ReduceMotion::System => current.reduce_motion,
        },
        ui_scale: a.ui_scale as f32,
        density: match a.grid_density {
            GridDensity::Compact => theme::Density::Compact,
            GridDensity::Comfortable => theme::Density::Comfortable,
            GridDensity::Large => theme::Density::Large,
        },
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
    }
}
