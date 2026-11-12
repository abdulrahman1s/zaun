//! Keycap hints ("Ctrl" "B") for menus, tooltips and the shortcut sheet.

use egui::{CornerRadius, Response, Sense, Stroke, StrokeKind, Ui, WidgetText, pos2, vec2};

use crate::theme::tokens::{radius, space, stroke};
use crate::theme::{self, Type};

/// Paint `combo` ("Ctrl+B") as keycaps. Decorative: hidden from assistive technology, since
/// the shortcut is part of the owning control's description.
pub fn key_hint(ui: &mut Ui, combo: &str) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let keys: Vec<&str> = combo.split('+').collect();
    let galleys: Vec<_> = keys
        .iter()
        .map(|k| {
            WidgetText::from(*k).into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                Type::Caption.font_id(),
            )
        })
        .collect();
    let cap_h = 20.0;
    let widths: Vec<f32> = galleys
        .iter()
        .map(|g| (g.size().x + space::S2 * 2.0 - 2.0).max(cap_h))
        .collect();
    let total = widths.iter().sum::<f32>() + space::S1 * (widths.len().saturating_sub(1)) as f32;
    let (rect, resp) = ui.allocate_exact_size(vec2(total, cap_h), Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let mut x = rect.left();
        for (g, w) in galleys.into_iter().zip(widths) {
            let r = egui::Rect::from_min_size(pos2(x, rect.top()), vec2(w, cap_h));
            let corner = CornerRadius::same(radius::XS);
            painter.rect_filled(r, corner, p.bg_raised);
            painter.rect_stroke(
                r,
                corner,
                Stroke::new(stroke::BORDER, p.border_strong),
                StrokeKind::Inside,
            );
            painter.galley(r.center() - g.size() / 2.0, g, p.text_secondary);
            x += w + space::S1;
        }
    }
    resp
}
