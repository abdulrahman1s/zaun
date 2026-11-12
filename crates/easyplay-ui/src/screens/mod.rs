//! Compose widgets into the screens of SCREEN_SPECS; one public `fn show(ui, vm, bridge, …)` per
//! screen file. No layout constants, business rules or strings here. See MODULES §8.

pub mod gallery;
pub mod library;
pub mod placeholder;
pub mod shell;

use easyplay_core::clock::{MonoInstant, Timestamp};

use crate::bridge::UiBridge;
use crate::textures::{ArtSource, EguiUploader, TextureCache};

/// Per-frame environment shared by every screen.
pub struct Env<'a> {
    /// Snapshots and commands.
    pub bridge: &'a UiBridge,
    /// Wall-clock now.
    pub now: Timestamp,
    /// Monotonic now.
    pub mono: MonoInstant,
    /// Local UTC offset (seconds) for date formatting.
    pub utc_offset_s: i32,
    /// Artwork textures.
    pub textures: &'a mut TextureCache<EguiUploader>,
    /// Where decoded artwork comes from.
    pub art: &'a dyn ArtSource,
}
