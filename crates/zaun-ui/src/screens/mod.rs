//! Compose widgets into the screens of SCREEN_SPECS; one public `fn show(ui, vm, bridge, …)` per
//! screen file. No layout constants, business rules or strings here. See MODULES §8.

pub mod actions;
pub mod add_game;
pub mod chat;
pub mod debug;
pub mod details;
pub mod dialogs;
pub mod flight;
pub mod gallery;
pub mod library;
pub mod onboarding;
pub mod quick_launch;
pub mod settings;
pub mod setup;
pub mod shell;
pub mod statistics;

use zaun_core::clock::{MonoInstant, Timestamp};

use crate::bridge::UiBridge;
use crate::textures::{ArtSource, EguiUploader, TextureCache};

/// Per-frame environment shared by every screen.
pub struct Env<'a> {
    /// Snapshots and commands.
    pub bridge: &'a mut UiBridge,
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
    /// The context (clipboard, viewport commands).
    pub ctx: egui::Context,
}

/// How far a reading column at most `column` wide is set in from the gutter, given the width
/// between the gutters: the column stands in the middle of a wider page, so a wide window
/// does not leave half of itself empty on one side. Zero while the page is no wider than
/// the column, where the page is laid out exactly as the design draws it.
pub fn centre_indent(between_gutters: f32, column: f32) -> f32 {
    ((between_gutters - column) / 2.0).max(0.0)
}
