//! The cover you open is the cover that arrives (UI_DESIGN_SYSTEM §5): going from the
//! library to a game page, the cover flies from its place on the wall to the page's cover
//! slot on a foreground layer, and back again. While it flies neither end paints it.
//!
//! The library and the game page only report where their covers are and ask whether to
//! paint them; the shell owns the flight.

use std::collections::HashMap;

use egui::Rect;
use zaun_core::ids::GameId;

/// A cover on its way between the wall and a game page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Flying {
    /// Whose cover.
    pub id: GameId,
    /// Where it left from.
    pub from: Rect,
    /// Where it is going.
    pub to: Rect,
    /// When it left (egui time).
    pub started: f64,
    /// Whether it is going to the game page (otherwise back to the wall).
    pub to_page: bool,
}

/// Where covers were drawn, and the one in flight.
#[derive(Clone, Debug, Default)]
pub struct Flight {
    /// Covers on the wall or in the list, in screen coordinates, as of the last pass that
    /// showed the library.
    pub(crate) wall: HashMap<GameId, Rect>,
    /// The game page's cover, as of the last pass that showed a game page.
    pub(crate) page: Option<(GameId, Rect)>,
    /// The cover in flight.
    pub(crate) flying: Option<Flying>,
}

impl Flight {
    /// The library drew (or would draw) the cover of `id` at `cover`. Call for every cover
    /// laid out on the wall or in the list, every pass.
    pub fn note_wall(&mut self, id: GameId, cover: Rect) {
        self.wall.insert(id, cover);
        if let Some(f) = &mut self.flying
            && f.id == id
            && !f.to_page
        {
            f.to = cover;
        }
    }

    /// The game page drew (or would draw) its cover at `cover`.
    pub fn note_page(&mut self, id: GameId, cover: Rect) {
        self.page = Some((id, cover));
        if let Some(f) = &mut self.flying
            && f.id == id
            && f.to_page
        {
            f.to = cover;
        }
    }

    /// Whether the cover of `id` is in flight: neither the wall nor the page paints it
    /// then (they still reserve its room and report its place).
    pub fn hides(&self, id: GameId) -> bool {
        self.flying.is_some_and(|f| f.id == id)
    }
}
