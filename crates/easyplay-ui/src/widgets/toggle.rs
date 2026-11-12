//! Toggle row: label left, optional description below, 36 × 20 track right; the whole row is
//! the hit target (§7.4).

use egui::{CornerRadius, Response, Sense, Stroke, StrokeKind, Ui, WidgetText, pos2, vec2};

use super::{focus_ring, hover_t};
use crate::a11y;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{alpha, control, radius, space, stroke};
use crate::theme::{self, Dur, Type};

/// A labelled switch. Returns the row's response; `changed()` is set when it flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str, description: Option<&str>) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let width = ui.available_width();
    let text_w = width - control::TOGGLE_W - space::S4;
    let label_g = WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        text_w,
        Type::Body.font_id(),
    );
    let desc_g = description.map(|d| {
        WidgetText::from(d).into_galley(
            ui,
            Some(egui::TextWrapMode::Wrap),
            text_w,
            Type::Caption.font_id(),
        )
    });
    let text_h = label_g.size().y + desc_g.as_ref().map_or(0.0, |g| g.size().y + space::S0);
    let h = text_h.max(control::MD);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let state = *on;
    let label_owned = label.to_owned();
    let enabled = resp.enabled();
    resp.widget_info(move || {
        egui::WidgetInfo::selected(egui::Role::Switch, enabled, state, &label_owned)
    });
    if let Some(d) = description {
        a11y::describe(&resp, d);
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let enabled = ui.is_enabled();
        let dim = |c| if enabled { c } else { fade(c, alpha::DISABLED) };
        let mut y = rect.center().y - text_h / 2.0;
        let lh = label_g.size().y;
        painter.galley(pos2(rect.left(), y), label_g, dim(p.text_primary));
        y += lh + space::S0;
        if let Some(g) = desc_g {
            painter.galley(pos2(rect.left(), y), g, dim(p.text_secondary));
        }
        let track = egui::Rect::from_center_size(
            pos2(rect.right() - control::TOGGLE_W / 2.0, rect.center().y),
            vec2(control::TOGGLE_W, control::TOGGLE_H),
        );
        let t = ui.ctx().animate_bool_with_time(
            resp.id.with("knob"),
            state,
            theme.motion.movement(Dur::Fast),
        );
        let hover = hover_t(&resp, &theme);
        let off_fill = mix(p.bg_raised, p.text_primary, 0.08 + 0.06 * hover);
        let fill = dim(mix(off_fill, p.accent, t));
        let corner = CornerRadius::same(radius::PILL);
        painter.rect_filled(track, corner, fill);
        if t < 1.0 || !p.allows_translucency() {
            painter.rect_stroke(
                track,
                corner,
                Stroke::new(stroke::BORDER, dim(fade(p.border_strong, 1.0 - t * 0.5))),
                StrokeKind::Inside,
            );
        }
        let knob_r = control::TOGGLE_H / 2.0 - 3.0;
        let x = egui::lerp(
            track.left() + 3.0 + knob_r..=track.right() - 3.0 - knob_r,
            t,
        );
        let knob = if state {
            p.text_on_accent
        } else {
            p.text_secondary
        };
        painter.circle_filled(
            pos2(x, track.center().y),
            knob_r,
            dim(mix(p.text_secondary, knob, t)),
        );
        if resp.has_focus() {
            focus_ring(painter, track, corner, &theme);
        }
    }
    resp
}
