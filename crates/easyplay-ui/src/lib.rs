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
pub mod input;
pub mod route;
pub mod screens;
pub mod strings;
pub mod textures;
pub mod theme;
pub mod vm;
pub mod widgets;

use std::sync::Arc;

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
}

/// `NativeOptions` for the main window: saved size, minimum 640 × 480, app id, icon.
pub fn native_options(window: &WindowGeometry, renderer: Renderer) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(strings::title::window(strings::library::TITLE))
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
        ..eframe::NativeOptions::default()
    }
}

/// Open the window and run until it closes.
pub fn run(start: StartMode, deps: UiDeps) -> eframe::Result {
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
    } = deps;
    let mut options = native_options(&window, renderer);
    if !open.is_empty() {
        options.viewport = options
            .viewport
            .with_inner_size(screens::quick_launch::SIZE)
            .with_min_inner_size(screens::quick_launch::SIZE)
            .with_resizable(false)
            .with_maximized(false)
            .with_title(strings::APP_NAME);
    }
    eframe::run_native(
        APP_ID,
        options,
        Box::new(move |cc| {
            let bridge = UiBridge::new(backend);
            let mut app = EasyPlayApp::new(bridge, clock, appearance, start.route(), utc_offset_s);
            if let Some(f) = art {
                app = app.with_art(f);
            }
            app = app.with_picker_factory(Box::new(|ctx| {
                let c = ctx.clone();
                Box::new(input::picker::RfdPicker::new(Arc::new(move || {
                    c.request_repaint();
                })))
            }));
            if let Some(store) = window_store {
                app = app.with_window_store(store, window);
            }
            if let Some(path) = open.into_iter().next() {
                app = app.quick_launch(path);
            }
            app.install(&cc.egui_ctx);
            Ok(Box::new(app))
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
