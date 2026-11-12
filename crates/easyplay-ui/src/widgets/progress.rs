//! Progress: linear bar (determinate or indeterminate) and ring spinner (§7.6).

use egui::accesskit::Role;
use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, vec2};

use crate::theme::palette::fade;
use crate::theme::tokens::{control, radius};
use crate::theme::{self};

/// A ring spinner at `center`. Requests a repaint while visible (it is the only continuous
/// animation, and only while something is busy). Under reduced motion it is a static arc.
pub fn spinner(ui: &Ui, center: Pos2, r: f32, color: Color32) {
    let theme = theme::theme(ui.ctx());
    let t = if theme.motion.reduced {
        0.0
    } else {
        ui.ctx().request_repaint();
        ui.input(|i| i.time) as f32
    };
    let start = t * std::f32::consts::TAU * 0.9;
    let n = 24;
    let sweep = std::f32::consts::TAU * 0.7;
    let points: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + sweep * i as f32 / n as f32;
            center + r * vec2(a.cos(), a.sin())
        })
        .collect();
    ui.painter().add(Shape::circle_stroke(
        center,
        r,
        Stroke::new(2.0, fade(color, 0.2)),
    ));
    ui.painter()
        .add(Shape::line(points, Stroke::new(2.0, color)));
}

/// A linear progress bar. `fraction` is `None` for indeterminate. `label` names it for
/// assistive technology ("Downloading, 312 megabytes so far").
pub fn linear(ui: &mut Ui, fraction: Option<f32>, label: &str) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, control::PROGRESS), Sense::hover());
    let theme = theme::theme(ui.ctx());
    let label = label.to_owned();
    resp.widget_info(move || {
        let mut info = egui::WidgetInfo::labeled(Role::ProgressIndicator, true, &label);
        info.value = fraction.map(|f| f64::from(f) * 100.0);
        info
    });
    paint_linear(
        ui,
        rect,
        fraction,
        theme.palette.accent,
        theme.palette.bg_raised,
    );
    resp
}

/// Paint a bar in `rect` (also used along a card's bottom edge).
pub fn paint_linear(ui: &Ui, rect: Rect, fraction: Option<f32>, fill: Color32, track: Color32) {
    let theme = theme::theme(ui.ctx());
    let corner = CornerRadius::same(radius::PILL);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, corner, track);
    match fraction {
        Some(f) => {
            let mut r = rect;
            r.set_width(rect.width() * f.clamp(0.0, 1.0));
            painter.rect_filled(r, corner, fill);
        }
        None => {
            let seg = rect.width() * 0.35;
            let t = if theme.motion.reduced {
                0.3
            } else {
                ui.ctx().request_repaint();
                (ui.input(|i| i.time) as f32 * 0.8).fract()
            };
            let x0 = rect.left() - seg + (rect.width() + seg) * t;
            let r = Rect::from_min_max(
                egui::pos2(x0.max(rect.left()), rect.top()),
                egui::pos2((x0 + seg).min(rect.right()), rect.bottom()),
            );
            if r.width() > 0.0 {
                painter.rect_filled(r, corner, fill);
            }
        }
    }
}
