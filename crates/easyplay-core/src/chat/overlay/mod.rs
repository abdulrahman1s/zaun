//! The in-game overlay (ADR-0037): what it is told and what it says over its pipes, the
//! snapshot it draws, and the process that draws it.
//!
//! - [`protocol`]: the versioned JSON lines.
//! - [`snapshot`]: [`OverlaySnapshot`] and its parts; [`build`] makes it from the Chat
//!   snapshot, [`notice`] carries notices, [`pictures`] sends each picture once.
//! - [`seam`]: [`Overlay`] and [`OverlayFactory`], what the service uses.
//! - [`status`]: [`OverlayStatus`], published for the hotkey and Settings.
//! - [`helper`]: `easyplay-overlay` under Bubblewrap.
//! - [`fake`]: an overlay for tests.

pub mod build;
pub mod fake;
pub mod helper;
pub mod notice;
pub mod pictures;
pub mod protocol;
pub mod seam;
pub mod snapshot;
pub mod status;

pub use helper::HelperOverlayFactory;
pub use protocol::{FromOverlay, ToOverlay};
pub use seam::{Overlay, OverlayEvent, OverlayFactory, OverlaySink};
pub use snapshot::{ChatKey, Corner, OverlayLook, OverlaySnapshot};
pub use status::OverlayStatus;

#[cfg(test)]
mod tests;
