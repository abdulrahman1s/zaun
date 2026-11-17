//! Keycaps ("/", "Ctrl" "B") for the search field, menus, tooltips and the shortcut list:
//! at least 17 × 17, 4 padding across, radius 4, a 1 pt `line` inner outline, 11 / 500 in
//! quiet text.

use egui::{
    Color32, CornerRadius, Galley, Painter, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
    pos2, vec2,
};
use std::sync::Arc;

use super::label_galley;
use crate::theme::tokens::{control, radius, space, stroke};
use crate::theme::{self, Theme, Type};

fn caps(painter: &Painter, combo: &str) -> Vec<(Arc<Galley>, f32)> {
    combo
        .split('+')
        .map(|k| {
            let g = label_galley(painter, k, Type::MicroStrong.font_id(), control::KBD);
            let w = (g.size().x + space::S1 * 2.0).max(control::KBD);
            (g, w)
        })
        .collect()
}

/// The size `combo` takes as keycaps.
pub fn key_hint_size(ui: &Ui, combo: &str) -> egui::Vec2 {
    let caps = caps(ui.painter(), combo);
    let w =
        caps.iter().map(|(_, w)| w).sum::<f32>() + space::S1 * caps.len().saturating_sub(1) as f32;
    vec2(w, control::KBD)
}

/// The keycaps of `combo` as shapes, from `left_center` rightwards, in `color` (quiet text,
/// or the tooltip's text at reduced strength).
pub fn key_hint_shapes(
    painter: &Painter,
    left_center: egui::Pos2,
    combo: &str,
    color: Color32,
    theme: &Theme,
) -> Vec<Shape> {
    let mut x = left_center.x;
    let mut out = Vec::new();
    for (g, w) in caps(painter, combo) {
        let r = Rect::from_min_size(
            pos2(x, left_center.y - control::KBD / 2.0),
            vec2(w, control::KBD),
        );
        out.push(Shape::rect_stroke(
            r,
            CornerRadius::same(radius::XS),
            Stroke::new(stroke::BORDER, theme.palette.line),
            StrokeKind::Inside,
        ));
        out.push(Shape::galley(r.center() - g.size() / 2.0, g, color));
        x += w + space::S1;
    }
    out
}

/// Paint `combo` ("Ctrl+B") as keycaps. Decorative: hidden from assistive technology, since
/// the shortcut is part of the owning control's description.
pub fn key_hint(ui: &mut Ui, combo: &str) -> Response {
    let theme = theme::theme(ui.ctx());
    let size = key_hint_size(ui, combo);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let shapes = key_hint_shapes(
            ui.painter(),
            pos2(rect.left(), rect.center().y),
            combo,
            theme.palette.t3,
            &theme,
        );
        ui.painter().extend(shapes);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keycap_is_at_least_17_square() {
        let r = crate::widgets::test_frame(&Default::default(), |ui| key_hint(ui, "/").rect);
        assert_eq!(r.size(), vec2(control::KBD, control::KBD));
    }
}
