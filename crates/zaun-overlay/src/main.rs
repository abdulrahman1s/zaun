//! `zaun-overlay` (ADR-0037): Zaun's in-game overlay. A fullscreen wlr-layer-shell
//! surface on the Overlay layer, drawn with egui through wgpu, above the game and outside
//! its sandbox. Zaun starts and stops it and talks to it over standard input and output
//! only (`zaun_core::chat::overlay::protocol`); games have no way to reach it.
//!
//! Exit status: 0 when Zaun closes its input, 2 without `WAYLAND_DISPLAY`, 3 when the
//! compositor has no layer shell (GNOME), 4 when nothing can draw a transparent surface.
mod app;
mod closed;
mod draw;
mod frame;
mod gpu;
mod handlers;
mod input;
mod keys;
mod labels;
mod layer;
mod look;
mod model;
mod motion;
mod open;
mod paint;
mod pictures;
mod pipe;
mod style;
mod surface;
mod words;

use std::process::ExitCode;

use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::shell::wlr_layer::LayerShell;
use wayland_client::Connection;
use wayland_client::globals::registry_queue_init;
use zaun_core::chat::overlay::protocol::{EXIT_NO_DISPLAY, EXIT_UNSUPPORTED};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ZAUN_OVERLAY_LOG")
                .unwrap_or_else(|_| "info,wgpu_core=warn,wgpu_hal=warn,naga=warn".into()),
        )
        .init();
    let code = match run() {
        Ok(code) => code,
        Err((code, why)) => {
            tracing::error!("overlay: {why}");
            code
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn run() -> Result<i32, (i32, String)> {
    // Never fall back to `wayland-0`: that could be another session's display.
    if std::env::var_os("WAYLAND_DISPLAY").is_none_or(|d| d.is_empty()) {
        return Err((EXIT_NO_DISPLAY, "WAYLAND_DISPLAY is not set".into()));
    }
    let conn = Connection::connect_to_env().map_err(|e| why(1, e))?;
    let (globals, queue) = registry_queue_init::<app::App>(&conn).map_err(|e| why(1, e))?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| why(1, e))?;
    let shell = LayerShell::bind(&globals, &qh).map_err(|e| why(EXIT_UNSUPPORTED, e))?;
    let mut event_loop: EventLoop<'static, app::App> =
        EventLoop::try_new().map_err(|e| why(1, e))?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue)
        .insert(handle.clone())
        .map_err(|e| why(1, e))?;
    pipe::listen(&handle).map_err(|e| why(1, e))?;
    let mut app = app::App::new(conn, &globals, qh, compositor, &shell, handle);
    while app.exit.is_none() {
        let timeout = app.timeout();
        event_loop
            .dispatch(timeout, &mut app)
            .map_err(|e| why(1, e))?;
        app.maybe_draw();
    }
    tracing::info!(frames = app.frames, "overlay: done");
    Ok(app.exit.map_or(0, i32::from))
}

fn why(code: i32, e: impl std::fmt::Display) -> (i32, String) {
    (code, e.to_string())
}
