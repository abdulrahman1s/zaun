//! Slider: a 4 pt track, the accent fill up to the thumb and `o3` after it, a 14 pt round
//! thumb in primary text, 150 wide and 20 high in the design. Drag, click or arrow keys
//! (`Home` / `End` for the ends).

use std::ops::RangeInclusive;

use egui::{CornerRadius, Key, Modifiers, Rect, Response, Sense, Ui, pos2, vec2};

use super::focus_ring;
use crate::theme::tokens::{control, radius};
use crate::theme::{self};

/// A horizontal slider over `range` in steps of `step`, `width` wide (the design's is
/// [`control::SLIDER_W`]). `value_text` is the spoken value ("100 %"). `changed()` is set
/// while the value moves; `drag_stopped()` / keyboard steps mark the end of an edit.
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
        ui.allocate_exact_size(vec2(width, control::SLIDER_H), Sense::click_and_drag());
    let thumb = control::SLIDER_THUMB;
    // The thumb's centre runs from half a thumb in from each end, as a range input's does.
    let run = (rect.left() + thumb / 2.0)..=(rect.right() - thumb / 2.0);
    let snap = |v: f32| ((v / step).round() * step).clamp(lo, hi);
    let before = *value;
    if let Some(pos) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let t = ((pos.x - run.start()) / (run.end() - run.start()).max(1.0)).clamp(0.0, 1.0);
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
        let t = if hi > lo {
            ((*value - lo) / (hi - lo)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let track =
            Rect::from_center_size(rect.center(), vec2(rect.width(), control::SLIDER_TRACK));
        // The fill runs to the same fraction of the whole track (CSS `--p`).
        let split = track.left() + track.width() * t;
        painter.rect_filled(
            Rect::from_min_max(track.min, pos2(split, track.max.y)),
            CornerRadius::ZERO,
            p.fill,
        );
        painter.rect_filled(
            Rect::from_min_max(pos2(split, track.min.y), track.max),
            CornerRadius::ZERO,
            p.o3,
        );
        let c = pos2(egui::lerp(run, t), rect.center().y);
        painter.circle_filled(c, thumb / 2.0, p.t1);
        if resp.has_focus() {
            focus_ring(
                painter,
                Rect::from_center_size(c, egui::Vec2::splat(thumb)),
                CornerRadius::same(radius::PILL),
                &theme,
            );
        }
    }
    resp
}
