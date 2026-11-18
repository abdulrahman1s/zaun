//! Capturing one game's window for a share (ADR-0036): the desktop's ScreenCast portal over
//! D-Bus picks the window (windows only, never a screen), PipeWire carries its pictures.
//! Built with the `portal` feature; without it EasyPlay cannot share (watching still works).

#[cfg(feature = "portal")]
mod pipewire;
#[cfg(feature = "portal")]
pub mod portal;

use std::sync::Arc;

use crate::video::source::Capture;

/// The capture of this build: the portal, or none.
pub fn system() -> Arc<dyn Capture> {
    #[cfg(feature = "portal")]
    {
        Arc::new(portal::PortalCapture::new())
    }
    #[cfg(not(feature = "portal"))]
    {
        Arc::new(crate::video::source::NoCapture)
    }
}
