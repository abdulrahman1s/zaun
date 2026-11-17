//! EasyPlay's user interface: present snapshots, collect intent, send commands.
//!
//! Depends only on the public API of `easyplay-core` listed in ARCHITECTURE §3 rule 2 plus
//! `egui`, `eframe`, `egui_extras`, `rfd` and `gilrs`. Performs no I/O (exceptions: `rfd`
//! dialogs on their own thread, and `textures` receiving decoded pixels from jobs), holds no
//! core lock across frames, contains no business rules, and keeps every user-facing string in
//! [`strings`]. The entry point is [`run`].

#![forbid(unsafe_code)]

pub mod a11y;
pub mod app;
pub mod bridge;
pub mod demo;
pub mod i18n;
pub mod input;
pub mod route;
pub mod screens;
pub mod service_backend;
pub mod strings;
pub mod textures;
pub mod theme;
pub mod vm;
pub mod widgets;

// Compiles every catalogue in `locales/` into the binary; English is the fallback.
rust_i18n::i18n!("locales", fallback = "en");

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use easyplay_core::clock::Clock;
use easyplay_core::consts::APP_ID;

pub use app::{EasyPlayApp, WindowGeometry, WindowStore};
pub use bridge::{Backend, UiBridge};

/// The bundled application icon (UI_DESIGN_SYSTEM §9).
pub static APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/app/easyplay-256.png");

/// Which route the window opens on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartMode {
    /// The library (normal start).
    #[default]
    Library,
    /// First launch.
    Onboarding,
    /// The widget gallery (development).
    Gallery,
}

impl StartMode {
    fn route(self) -> route::Route {
        match self {
            StartMode::Library => route::Route::Library,
            StartMode::Onboarding => route::Route::Onboarding,
            StartMode::Gallery => route::Route::Gallery,
        }
    }
}

/// The renderer (V-301): `wgpu` by default, `glow` as the fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Renderer {
    /// wgpu (Vulkan / GL through wgpu).
    #[default]
    Wgpu,
    /// OpenGL through glow.
    Glow,
}

/// Which windowing backend winit uses (chosen by the binary before the event loop exists,
/// from which client libraries can be loaded; the event loop cannot be created twice).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisplayBackend {
    /// winit's choice: Wayland when `WAYLAND_DISPLAY` is set, else X11.
    #[default]
    Auto,
    /// X11 (Xwayland), e.g. when `libwayland-client` cannot be loaded.
    X11,
}

/// A request from another EasyPlay process to show this window (single instance,
/// DESKTOP_INTEGRATION §5.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activation {
    /// The desktop activation token the other process was started with (informational:
    /// egui cannot pass it to the compositor; focus falls back to an attention request).
    pub token: Option<String>,
    /// Open the add-game picker afterwards (`easyplay --add`).
    pub add_game: bool,
}

/// A repaint waker for other threads.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Receives the [`Waker`] once the window exists.
pub type WakerSink = Box<dyn FnOnce(Waker) + Send>;

/// Called as the window closes, with the activations it did not get to (the resident service
/// opens a window for them later).
pub type CloseSink = Box<dyn FnOnce(Vec<Activation>) + Send>;

/// Everything the UI needs from the binary.
pub struct UiDeps {
    /// The application service seam.
    pub backend: Box<dyn Backend>,
    /// Time source.
    pub clock: Arc<dyn Clock>,
    /// Appearance preferences (from `config.toml`).
    pub appearance: theme::Appearance,
    /// Saved window geometry (from `window.json`).
    pub window: WindowGeometry,
    /// Where geometry is saved; `None` disables saving.
    pub window_store: Option<Box<dyn WindowStore>>,
    /// Local UTC offset in seconds (date formatting without I/O on the UI thread).
    pub utc_offset_s: i32,
    /// Renderer.
    pub renderer: Renderer,
    /// Artwork source factory; `None` paints placeholders only.
    pub art: Option<app::ArtFactory>,
    /// A program opened from the file manager: start in the quick-launch layout
    /// (SCREEN_SPECS §7). Empty for a normal start.
    pub open: Vec<std::path::PathBuf>,
    /// The programs of [`UiDeps::open`] were already sent to the service (the resident
    /// service does so before the window exists); the window only follows them.
    pub open_sent: bool,
    /// Open the add-game picker once the window is up (`easyplay --add`).
    pub add_game: bool,
    /// Raise requests from other processes; the sender wakes the UI with the waker handed
    /// to [`UiDeps::on_waker`].
    pub activations: Option<Receiver<Activation>>,
    /// Kept equal to "the window has keyboard focus" (notifications are sent only when not).
    pub focus: Option<Arc<AtomicBool>>,
    /// Receives a repaint waker once the window exists (other threads wake the UI with it).
    pub on_waker: Option<WakerSink>,
    /// Windowing backend.
    pub display: DisplayBackend,
    /// The desktop's appearance preferences now, and their changes (A11Y-004).
    pub system: theme::SystemPrefs,
    /// Changes of [`UiDeps::system`]; the sender wakes the UI with the waker of
    /// [`UiDeps::on_waker`].
    pub system_changes: Option<Receiver<theme::SystemPrefs>>,
    /// Called as the window closes.
    pub on_close: Option<CloseSink>,
}

/// Whether `e` happened while creating winit's event loop (no display, client libraries
/// missing). The event loop exists once per process, so another renderer cannot help.
pub fn is_event_loop_error(e: &eframe::Error) -> bool {
    matches!(e, eframe::Error::WinitEventLoop(_))
}

/// Raises the window for [`Activation`]s and mirrors focus; wraps the application.
struct Shell {
    app: EasyPlayApp,
    activations: Option<Receiver<Activation>>,
    focus: Option<Arc<AtomicBool>>,
    /// When to ask for attention if focus did not arrive (seconds of `ctx` time).
    attention_at: Option<f64>,
    on_close: Option<CloseSink>,
}

/// How long to wait for focus before requesting attention instead.
const FOCUS_GRACE_S: f64 = 0.5;

impl Shell {
    fn activate(&mut self, ctx: &egui::Context, a: Activation) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.attention_at = Some(ctx.input(|i| i.time) + FOCUS_GRACE_S);
        if a.add_game {
            screens::add_game::open(&mut self.app.state);
        }
        ctx.request_repaint();
    }
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let got: Vec<Activation> = self
            .activations
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for a in got {
            self.activate(ctx, a);
        }
        let (focused, now) = ctx.input(|i| (i.focused, i.time));
        if let Some(f) = &self.focus {
            f.store(focused, Ordering::Relaxed);
        }
        if let Some(t) = self.attention_at {
            if focused {
                self.attention_at = None;
            } else if now >= t {
                // The compositor refused focus without a usable token.
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                    egui::UserAttentionType::Informational,
                ));
                self.attention_at = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(t - now));
            }
        }
        eframe::App::logic(&mut self.app, ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.app, ctx, raw_input);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        eframe::App::clear_color(&self.app, visuals)
    }

    fn persist_egui_memory(&self) -> bool {
        eframe::App::persist_egui_memory(&self.app)
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        eframe::App::on_exit(&mut self.app, gl);
        if let Some(f) = self.on_close.take() {
            let unread = self
                .activations
                .take()
                .map(|rx| rx.try_iter().collect())
                .unwrap_or_default();
            f(unread);
        }
    }
}

/// `NativeOptions` for the main window: saved size, minimum 640 × 480, app id, icon.
pub fn native_options(window: &WindowGeometry, renderer: Renderer) -> eframe::NativeOptions {
    native_options_on(window, renderer, DisplayBackend::Auto)
}

/// [`native_options`] on a chosen windowing backend.
pub fn native_options_on(
    window: &WindowGeometry,
    renderer: Renderer,
    display: DisplayBackend,
) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(strings::title::window(strings::library::TITLE()))
        .with_app_id(APP_ID)
        .with_inner_size(window.size)
        .with_min_inner_size(theme::tokens::layout::MIN_WINDOW)
        .with_maximized(window.maximized);
    if let Some(p) = window.position {
        viewport = viewport.with_position(p);
    }
    if let Ok(icon) = eframe::icon_data::from_png_bytes(APP_ICON_PNG) {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    eframe::NativeOptions {
        viewport,
        renderer: match renderer {
            Renderer::Wgpu => eframe::Renderer::Wgpu,
            Renderer::Glow => eframe::Renderer::Glow,
        },
        persist_window: false,
        system_font_fallback: true,
        event_loop_builder: match display {
            DisplayBackend::Auto => None,
            DisplayBackend::X11 => Some(Box::new(|b| {
                use winit::platform::x11::EventLoopBuilderExtX11 as _;
                b.with_x11();
            })),
        },
        ..eframe::NativeOptions::default()
    }
}

/// Whether [`run`] shows onboarding in its own compact window: first launch, with nothing
/// opened. The window closes when onboarding finishes; the caller then opens the library.
pub fn opens_onboarding(deps: &UiDeps) -> bool {
    deps.open.is_empty()
        && !deps.add_game
        && !deps.backend.is_preview()
        && !deps.backend.preferences().onboarding.completed
}

/// Open the window and run until it closes.
pub fn run(start: StartMode, deps: UiDeps) -> eframe::Result {
    let onboarding = start == StartMode::Library && opens_onboarding(&deps);
    let UiDeps {
        backend,
        clock,
        appearance,
        window,
        window_store,
        utc_offset_s,
        renderer,
        art,
        open,
        add_game,
        activations,
        focus,
        on_waker,
        display,
        system,
        system_changes,
        open_sent,
        on_close,
    } = deps;
    let mut options = native_options_on(&window, renderer, display);
    if let Some(path) = open.first() {
        let size = screens::quick_launch::window_size(&backend.library(), path);
        options.viewport = options
            .viewport
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_resizable(false)
            .with_maximized(false)
            .with_title(strings::APP_NAME);
    } else if onboarding {
        let size = screens::onboarding::WINDOW_SIZE;
        options.viewport = options
            .viewport
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_resizable(false)
            .with_maximized(false)
            .with_title(strings::APP_NAME);
    }
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            let bridge = UiBridge::new(backend);
            let mut app = EasyPlayApp::new(bridge, clock, appearance, start.route(), utc_offset_s)
                .with_system_prefs(system, system_changes);
            if let Some(f) = art {
                app = app.with_art(f);
            }
            app = app.with_picker_factory(Box::new(|ctx| {
                let c = ctx.clone();
                Box::new(input::picker::RfdPicker::new(Arc::new(move || {
                    c.request_repaint();
                })))
            }));
            // The compact onboarding window's size is not the library's.
            if let Some(store) = window_store.filter(|_| !onboarding) {
                app = app.with_window_store(store, window);
            }
            app.state.onboarding_window = onboarding;
            if let Some(path) = open.into_iter().next() {
                app = if open_sent {
                    app.follow_quick_launch(path)
                } else {
                    app.quick_launch(path)
                };
            }
            if add_game {
                screens::add_game::open(&mut app.state);
            }
            #[cfg(feature = "gamepad")]
            {
                let c = cc.egui_ctx.clone();
                if let Some(rx) = input::gamepad::spawn(Arc::new(move || c.request_repaint())) {
                    app = app.with_gamepad(rx);
                }
            }
            app.install(&cc.egui_ctx);
            if let Some(f) = on_waker {
                let c = cc.egui_ctx.clone();
                f(Arc::new(move || c.request_repaint()));
            }
            Ok(Box::new(Shell {
                app,
                activations,
                focus,
                attention_at: None,
                on_close,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
    /// Smoke test: the UI links against the core model and egui.
    #[test]
    fn links_core_and_egui() {
        assert_eq!(easyplay_core::model::BasePreset::ALL.len(), 3);
        let ctx = egui::Context::default();
        assert!(ctx.pixels_per_point() > 0.0);
    }

    #[test]
    fn app_icon_decodes() {
        assert!(eframe::icon_data::from_png_bytes(super::APP_ICON_PNG).is_ok());
    }
}
