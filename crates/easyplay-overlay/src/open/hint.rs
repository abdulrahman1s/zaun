//! The hint 16 under the panel, centred: "Press [Shift+F1] or [Esc] to go back to the
//! game", 13 t2, the keys as 20-high key caps (12 medium t1, a 1-point line2 ring, radius 5).
//! Without a key (`chat.overlay_key` empty): "[Esc] Back to the game". The parts are made
//! once per snapshot (`labels::Labels::back`).
use easyplay_ui::theme::Type;
use egui::{Align2, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::model::Model;
use crate::paint::text;
use crate::words;

const SPACE: f32 = 6.0;

pub fn draw(ui: &Ui, m: &Model, top_centre: Pos2) {
    let look = &m.look;
    let galley = |cap: bool, s: &str| {
        if cap {
            text::galley(ui, Type::Label, s, look.t1, 200.0, 1)
        } else {
            text::galley(ui, Type::Body, s, look.t2, 600.0, 1)
        }
    };
    let parts = &m.labels.back;
    let width: f32 = parts
        .iter()
        .map(|p| galley(p.cap, &p.text).size().x + if p.cap { 12.0 } else { 0.0 })
        .sum::<f32>()
        + SPACE * (parts.len().saturating_sub(1)) as f32;
    let rtl = words::rtl();
    let mut x = if rtl {
        top_centre.x + width / 2.0
    } else {
        top_centre.x - width / 2.0
    };
    let y = top_centre.y + 10.0;
    for p in parts {
        let g = galley(p.cap, &p.text);
        let w = g.size().x + if p.cap { 12.0 } else { 0.0 };
        let left = if rtl { x - w } else { x };
        let rect = Rect::from_min_size(pos2(left, y - 10.0), vec2(w, 20.0));
        if p.cap {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(5),
                Stroke::new(1.0, look.line2),
                StrokeKind::Inside,
            );
        }
        text::put(ui, &g, rect.center(), Align2::CENTER_CENTER);
        x = if rtl { left - SPACE } else { left + w + SPACE };
    }
}
