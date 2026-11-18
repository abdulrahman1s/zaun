//! The layer surface: fullscreen on the Overlay layer, and what "open" and "closed" mean to
//! the compositor (ADR-0037 §2).
//!
//! - **Closed:** an empty input region (every click and key goes to the game) and no
//!   keyboard. Toasts and the call widget are drawn but cannot be clicked.
//! - **Open:** the whole surface takes input and the keyboard is exclusive, so typing reaches
//!   the panel and not the game.
//!
//! No output is named: the compositor puts a new layer surface on the output it considers
//! current (the focused one on niri, Sway and Hyprland).
use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerSurface,
};
use wayland_client::QueueHandle;

use crate::app::App;

/// The namespace compositors see (window rules can match it).
pub const NAMESPACE: &str = "zaun-overlay";

/// A fullscreen, closed overlay surface, committed without a buffer (the compositor answers
/// with a configure; the first frame is drawn after it).
pub fn create(
    compositor: &CompositorState,
    shell: &LayerShell,
    qh: &QueueHandle<App>,
) -> LayerSurface {
    let surface = compositor.create_surface(qh);
    let layer = shell.create_layer_surface(qh, surface, Layer::Overlay, Some(NAMESPACE), None);
    layer.set_anchor(Anchor::all());
    layer.set_size(0, 0);
    // Cover panels and bars too.
    layer.set_exclusive_zone(-1);
    set_input(&layer, compositor, false);
    layer.commit();
    layer
}

/// Take or give back input. Applied with the next commit (the next frame).
pub fn set_input(layer: &LayerSurface, compositor: &CompositorState, open: bool) {
    if open {
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.wl_surface().set_input_region(None);
    } else {
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        match Region::new(compositor) {
            Ok(empty) => layer.wl_surface().set_input_region(Some(empty.wl_region())),
            Err(e) => tracing::warn!(error = %e, "overlay: no empty input region"),
        }
    }
}
