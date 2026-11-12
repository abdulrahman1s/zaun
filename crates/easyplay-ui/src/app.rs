//! `EasyPlayApp` implementing `eframe::App`: `logic` (drain events, tick view-models),
//! `ui` (shell + current route + overlays), `raw_input_hook` (gamepad and drops arrive with
//! UI-031 / A11Y), `on_exit` (save geometry). See MODULES §8 and task UI-001.
//!
//! [`EasyPlayApp::frame`] is the whole per-frame UI; the eframe `ui` hook and the kittest
//! harness both call it, so tests exercise exactly what the window shows.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::Clock;
use easyplay_core::ids::GameId;
use egui::{Context, Ui};

use crate::bridge::{UiBridge, UiEvent};
use crate::route::{Navigator, Route};
use crate::screens::gallery::GalleryState;
use crate::screens::shell::{Banner, BannerQueue};
use crate::screens::{Env, shell};
use crate::strings;
use crate::textures::{ArtSource, EguiUploader, Limits, TextureCache};
use crate::theme::tokens::timing;
use crate::theme::{self, Appearance};
use crate::vm::library::LibraryVm;
use crate::vm::toasts::{ToastKind, ToastQueue};
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
    /// The last window title sent.
    pub last_title: Option<String>,
}

impl UiState {
    /// Fresh state starting at `route`.
    pub fn new(route: Route) -> Self {
        Self {
            nav: Navigator::new(route),
            library: LibraryVm::default(),
            toasts: ToastQueue::new(),
            sidebar_pref: None,
            banners: BannerQueue::default(),
            gallery: GalleryState::default(),
            pending_focus: None,
            focus_search: false,
            last_title: None,
        }
    }
}

/// Creates the artwork source once the context exists (so it can wake the UI).
pub type ArtFactory = Box<dyn FnOnce(&Context) -> Box<dyn ArtSource>>;

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
        ctx.options_mut(|o| o.zoom_with_keyboard = true);
        self.installed = true;
    }

    /// The bridge (tests).
    pub fn bridge(&self) -> &UiBridge {
        &self.bridge
    }

    /// Per-frame logic: drain events, tick toasts. Never blocks.
    pub fn logic(&mut self, ctx: &Context) {
        self.install(ctx);
        let mono = self.clock.mono();
        for ev in self.bridge.drain() {
            match ev {
                UiEvent::Toast(text) => {
                    self.state.toasts.push(text, ToastKind::Info, None, mono);
                }
                UiEvent::LibraryChanged | UiEvent::HostChanged => {}
            }
        }
        if let Some(next) = self.state.toasts.tick(mono) {
            ctx.request_repaint_after(next.max(Duration::from_millis(16)));
        }
        self.track_geometry(ctx);
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
        let mut env = Env {
            bridge: &self.bridge,
            now: self.clock.now(),
            mono: self.clock.mono(),
            utc_offset_s: self.utc_offset_s,
            textures,
            art,
        };
        shell::show(ui, &mut self.state, &mut env);
        env.textures.end_frame(art);
        if env.textures.in_flight() > 0 || env.textures.waiting_upload() > 0 {
            // Results arrive through the source's waker; pending uploads need another frame.
            if env.textures.waiting_upload() > 0 {
                ctx.request_repaint();
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
