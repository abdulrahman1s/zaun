//! Slider (§7.4): a 4 px rail, the filled part in `accent`, a round 18 px knob in
//! `bg.raised` with a `border.strong` outline; drag, click or arrow keys (`Home`/`End` for the
//! ends). egui's own slider paints rail and knob with the same widget fill, which left the
//! rail invisible on the light theme (UX-004).

use std::ops::RangeInclusive;

use egui::{CornerRadius, Key, Modifiers, Response, Sense, Stroke, Ui, pos2, vec2};

use super::{focus_ring, hover_t};
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{alpha, control, radius, stroke};
use crate::theme::{self};

const RAIL_H: f32 = 4.0;
const KNOB: f32 = 18.0;

/// A horizontal slider over `range` in steps of `step`, `width` wide. `value_text` is the
/// spoken and shown value ("100 %"). `changed()` is set while the value moves;
/// `drag_stopped()` / keyboard steps mark the end of an edit.
pub fn slider(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step: f32,
    width: f32,
    label: &str,
    value_text: &str,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let (lo, hi) = (*range.start(), *range.end());
    let (rect, mut resp) =
        ui.allocate_exact_size(vec2(width, control::MD), Sense::click_and_drag());
    let rail = egui::Rect::from_center_size(rect.center(), vec2(rect.width() - KNOB, RAIL_H));
    let snap = |v: f32| ((v / step).round() * step).clamp(lo, hi);
    let before = *value;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let t = ((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0);
        *value = snap(lo + t * (hi - lo));
    }
    if resp.has_focus() {
        super::own_arrow_keys(&resp);
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::ArrowLeft)
                || i.consume_key(Modifiers::NONE, Key::ArrowDown)
            {
                *value = snap(*value - step);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowRight)
                || i.consume_key(Modifiers::NONE, Key::ArrowUp)
            {
                *value = snap(*value + step);
            }
            if i.consume_key(Modifiers::NONE, Key::Home) {
                *value = lo;
            }
            if i.consume_key(Modifiers::NONE, Key::End) {
                *value = hi;
            }
        });
    }
    if (*value - before).abs() > f32::EPSILON {
        resp.mark_changed();
    }
    let enabled = resp.enabled();
    let (label, text, v) = (label.to_owned(), value_text.to_owned(), *value);
    resp.widget_info(move || {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, enabled, &label);
        info.value = Some(f64::from(v));
        info.current_text_value = Some(text.clone());
        info
    });

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let dim = |c| if enabled { c } else { fade(c, alpha::DISABLED) };
        let t = if hi > lo {
            (*value - lo) / (hi - lo)
        } else {
            0.0
        };
        let knob_x = rail.left() + t * rail.width();
        let corner = CornerRadius::same(radius::PILL);
        painter.rect_filled(rail, corner, dim(mix(p.bg_raised, p.text_primary, 0.14)));
        let filled = egui::Rect::from_min_max(rail.min, pos2(knob_x, rail.max.y));
        painter.rect_filled(filled, corner, dim(p.accent));
        let hover = hover_t(&resp, &theme);
        let c = pos2(knob_x, rail.center().y);
        let r = KNOB / 2.0 + hover;
        painter.circle(
            c,
            r,
            dim(p.bg_raised),
            Stroke::new(
                stroke::BORDER + hover,
                dim(mix(p.border_strong, p.accent, hover)),
            ),
        );
        if resp.has_focus() {
            focus_ring(
                painter,
                egui::Rect::from_center_size(c, egui::Vec2::splat(KNOB)),
                CornerRadius::same(radius::PILL),
                &theme,
            );
        }
    }
    resp
}
