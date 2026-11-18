//! Progress: the spinner (something is starting), the running game's "live" mark, and a
//! linear bar (a download).
//!
//! The spinner is a 14 pt ring, 2 pt wide, a quarter of it open, one turn in 700 ms (1.6 s
//! under reduced motion). The live mark is three 2 pt bars, 9 high, 2 apart, that rise and
//! fall between 35 % and their full height over 900 ms, each 300 ms ahead of the one before;
//! under reduced motion they stand still at full height. Both repaint only while shown.

use egui::accesskit::Role;
use egui::{Color32, Context, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, vec2};

use crate::theme::motion::cubic_bezier;
use crate::theme::tokens::{control, radius, stroke, timing};
use crate::theme::{self};

/// The spinner's ring as a shape, centred at `center` with outer radius `r`: three quarters
/// of a circle, turning. Asks for the next frame.
pub fn spinner_shape(ctx: &Context, center: Pos2, r: f32, color: Color32) -> Shape {
    let theme = theme::theme(ctx);
    let period = if theme.motion.reduced {
        timing::SPIN_REDUCED
    } else {
        timing::SPIN
    };
    ctx.request_repaint();
    let now = ctx.input(|i| i.time);
    let turn = ((now / f64::from(period)).fract()) as f32;
    let tau = std::f32::consts::TAU;
    // The open quarter is the ring's right side at rest (CSS `border-right-color:
    // transparent`), turning clockwise with the ring.
    let start = turn * tau + tau / 8.0;
    let sweep = tau * 0.75;
    let w = stroke::SPINNER;
    let radius = (r - w / 2.0).max(0.5);
    let n = 28;
    let points: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + sweep * i as f32 / n as f32;
            center + radius * vec2(a.cos(), a.sin())
        })
        .collect();
    Shape::line(points, Stroke::new(w, color))
}

/// Paint the spinner at `center`, outer radius `r` (half of 14 pt in the design).
pub fn spinner(ui: &Ui, center: Pos2, r: f32, color: Color32) {
    ui.painter().add(spinner_shape(ui.ctx(), center, r, color));
}

/// A spinner standing in the layout: 14 pt, in `color`.
pub fn spinner_widget(ui: &mut Ui, color: Color32) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(control::SPINNER, control::SPINNER), Sense::hover());
    if ui.is_rect_visible(rect) {
        spinner(ui, rect.center(), control::SPINNER / 2.0, color);
    }
    resp
}

/// The size of the live mark.
pub fn live_size() -> egui::Vec2 {
    let [bar, gap, h] = control::LIVE_BAR;
    vec2(bar * 3.0 + gap * 2.0, h)
}

/// The live mark's bars as shapes, standing on the bottom of `rect` (see [`live_size`]).
/// Asks for the next frame while it moves.
pub fn live_shapes(ctx: &Context, rect: Rect, color: Color32) -> Vec<Shape> {
    let theme = theme::theme(ctx);
    let [bar, gap, h] = control::LIVE_BAR;
    let now = ctx.input(|i| i.time);
    // A game is played in another window most of the time: the bars move only while this
    // window says it is in front, and thirty frames a second are plenty for them.
    let moving = theme.motion.shimmer() && ctx.input(|i| i.viewport().focused == Some(true));
    if moving {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    (0..3_u8)
        .map(|i| {
            let scale = if moving {
                let beat = f64::from(timing::LIVE);
                // Each bar runs `i` thirds of a beat ahead (negative CSS delays).
                let phase = (now + f64::from(i) * beat / 3.0) / beat;
                let p = (phase % 2.0) as f32;
                let x = if p < 1.0 { p } else { 2.0 - p };
                // CSS `ease-in-out`, alternating.
                let eased = cubic_bezier(0.42, 0.0, 0.58, 1.0, x);
                control::LIVE_LOW + (1.0 - control::LIVE_LOW) * eased
            } else if theme.motion.reduced {
                // Under reduced motion the bars stand at full height.
                1.0
            } else {
                // At rest the bars stand at three heights, so the mark still reads as one.
                [0.6, 1.0, 0.45][usize::from(i)]
            };
            let x = rect.left() + f32::from(i) * (bar + gap);
            let bh = h * scale;
            let r = Rect::from_min_max(
                egui::pos2(x, rect.bottom() - bh),
                egui::pos2(x + bar, rect.bottom()),
            );
            Shape::rect_filled(r, CornerRadius::same(radius::BAR), color)
        })
        .collect()
}

/// The running game's live mark standing in the layout, in `color` (the accent text in the
/// top bar).
pub fn live(ui: &mut Ui, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(live_size(), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().extend(live_shapes(ui.ctx(), rect, color));
    }
    resp
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
    paint_linear(ui, rect, fraction, theme.palette.fill, theme.palette.o3);
    resp
}

/// Paint a bar in `rect`: `fill` up to the fraction over `track`.
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
            // Under reduced motion the stretch stands still in the middle.
            let t = if theme.motion.reduced {
                0.5
            } else {
                ui.ctx().request_repaint();
                (ui.input(|i| i.time) as f32 / timing::SHIMMER_PERIOD).fract()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_mark_is_three_bars_nine_high() {
        assert_eq!(live_size(), vec2(10.0, 9.0));
    }

    #[test]
    fn bars_stand_at_full_height_under_reduced_motion() {
        let appearance = theme::Appearance {
            reduce_motion: true,
            ..Default::default()
        };
        let shapes = crate::widgets::test_frame(&appearance, |ui| {
            live_shapes(
                ui.ctx(),
                Rect::from_min_size(Pos2::ZERO, live_size()),
                Color32::WHITE,
            )
        });
        for s in shapes {
            assert_eq!(s.visual_bounding_rect().height(), control::LIVE_BAR[2]);
        }
    }
}
