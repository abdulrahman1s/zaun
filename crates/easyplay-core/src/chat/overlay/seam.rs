//! The seam between the service and the overlay process (ADR-0037): the service sends
//! [`ToOverlay`] messages and hears [`OverlayEvent`]s. The `easyplay-overlay` helper is the
//! one implementation ([`super::helper`]); tests use [`super::fake::FakeOverlay`].
use std::sync::Arc;
use std::time::Duration;

use super::protocol::{FromOverlay, ToOverlay};
use crate::error::Result;

/// What a running overlay reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    /// A message from the overlay.
    Said(FromOverlay),
    /// It speaks another protocol version (an overlay of another build): it is stopped and
    /// not started again.
    Incompatible(u32),
    /// The process ended. `ready` = it had said [`FromOverlay::Ready`].
    Exited { status: Option<i32>, ready: bool },
}

/// Where events go: called on the overlay's reader thread.
pub type OverlaySink = Arc<dyn Fn(OverlayEvent) + Send + Sync>;

/// A running overlay. Dropping it stops the process.
pub trait Overlay: Send {
    /// Queue a message (never blocks).
    fn send(&self, msg: &ToOverlay);
}

/// Starts overlays.
pub trait OverlayFactory: Send + Sync {
    /// Whether this build and session can show one (the helper is there, and a Wayland
    /// display is known).
    fn available(&self) -> bool;
    /// Start an overlay; events go to `sink` until it is dropped or exits.
    fn start(&self, sink: OverlaySink) -> Result<Box<dyn Overlay>>;
    /// Whether the desktop offers the layer shell the overlay needs (`None` = cannot tell).
    /// It talks to the compositor: call it off the service thread, once.
    fn has_layer_shell(&self) -> Option<bool> {
        None
    }
    /// How long to really wait before a restart after a back-off of `wait` (test overlays
    /// shorten it).
    fn restart_wait(&self, wait: Duration) -> Duration {
        wait
    }
}
