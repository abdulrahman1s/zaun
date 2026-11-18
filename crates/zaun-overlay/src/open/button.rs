//! The panel's buttons: flat plates that light up under the pointer (o1, o2 when pressed),
//! each a keyboard stop with a tooltip.
use egui::{Color32, CornerRadius, Id, Rect, Response, Sense, Ui};

use crate::look::Look;

/// A clickable rect with the hover plate; `tip` is shown after the usual delay.
pub fn plate(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    radius: u8,
    look: &Look,
    idle: Color32,
    tip: &str,
) -> Response {
    let r = ui.interact(rect, id, Sense::click());
    let lit = r.hovered() || r.is_pointer_button_down_on();
    let fill = match (lit, idle) {
        (false, c) => c,
        (true, c) if c == Color32::TRANSPARENT => look.o1,
        (true, c) if c == look.fill => look.fill_hover,
        (true, c) if c == look.coral => c.gamma_multiply(1.1),
        (true, _) => look.o3,
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius), fill);
    if r.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(radius.saturating_add(2)),
            egui::Stroke::new(1.5, look.amark),
            egui::StrokeKind::Outside,
        );
    }
    if tip.is_empty() {
        r
    } else {
        r.on_hover_text(tip)
    }
}
