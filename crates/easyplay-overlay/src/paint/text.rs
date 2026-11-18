//! Text as the window lays it out: the app's type scale, bidirectional lines and Arabic
//! shaping (`easyplay_ui::theme::Type::galley`), cut to one line with an ellipsis where a
//! design says so.
use std::sync::Arc;

use easyplay_ui::theme::Type;
use egui::{Align2, Color32, Galley, Pos2, Ui};

/// `text` in `t`, at most `rows` rows `width` wide.
pub fn galley(
    ui: &Ui,
    t: Type,
    text: &str,
    colour: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    t.galley(ui, text, colour, width.max(1.0), rows)
}

/// Paint a galley with its `anchor` corner at `at` (the anchor is flipped for RTL by the
/// caller where the design mirrors).
pub fn put(ui: &Ui, g: &Arc<Galley>, at: Pos2, anchor: Align2) -> egui::Rect {
    let rect = anchor.anchor_size(at, g.size());
    ui.painter()
        .galley(rect.min, Arc::clone(g), Color32::PLACEHOLDER);
    rect
}

/// One line of `text` in `t` with its start (its end in RTL) at `at`'s x and its top at
/// `at`'s y; returns its rect.
pub fn line(
    ui: &Ui,
    t: Type,
    text: &str,
    colour: Color32,
    at: Pos2,
    width: f32,
    rtl: bool,
) -> egui::Rect {
    let g = galley(ui, t, text, colour, width, 1);
    let anchor = if rtl {
        Align2::RIGHT_TOP
    } else {
        Align2::LEFT_TOP
    };
    put(ui, &g, at, anchor)
}
