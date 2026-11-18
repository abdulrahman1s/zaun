//! The switch: a 32 × 20 track that fills with the accent when on, and a 14 pt knob that
//! travels 12 (220 ms; the colours change linearly, the knob moves with `ease_out`).
//!
//! [`switch`] is the bare control the design's setting rows hold; [`toggle`] is a whole row
//! of its own (label, help, the switch at the right end) for the places that still lay one
//! out themselves.

use egui::{CornerRadius, Rect, Response, Sense, Ui, pos2, vec2};

use super::focus_ring;
use crate::a11y;
use crate::theme::motion::glide;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, layout, radius};
use crate::theme::{self, Dur, Type};

/// A bare switch named `name` (its setting row's label). Returns its response; `changed()`
/// when it flipped.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> Response {
    let (rect, mut resp) =
        ui.allocate_exact_size(vec2(control::TOGGLE_W, control::TOGGLE_H), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    a11y_switch(&resp, name, *on);
    if ui.is_rect_visible(rect) {
        paint_track(ui, &resp, rect, *on);
        if resp.has_focus() {
            let theme = theme::theme(ui.ctx());
            focus_ring(ui.painter(), rect, CornerRadius::same(radius::PILL), &theme);
        }
    }
    resp
}

fn a11y_switch(resp: &Response, name: &str, on: bool) {
    let enabled = resp.enabled();
    let name = name.to_owned();
    resp.widget_info(move || egui::WidgetInfo::selected(egui::Role::Switch, enabled, on, &name));
}

/// A labelled switch as a row of its own: the label (and optional help under it) at the
/// left, the switch at the right end; the whole row is the target. Returns the row's
/// response; `changed()` when it flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str, description: Option<&str>) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let width = ui.available_width();
    let text_w = (width - control::TOGGLE_W - layout::SETTING_GAP).max(1.0);
    let label_g = Type::Body.galley(ui, label, p.t1, text_w, usize::MAX);
    let desc_g =
        description.map(|d| super::rows::help_galley(ui, d, p.t3, text_w.min(layout::HELP_MAX_W)));
    let text_h = label_g.size().y + desc_g.as_ref().map_or(0.0, |g| g.size().y);
    let h = (text_h + layout::FACT_PAD_Y * 2.0).max(control::SETTING_ROW);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    a11y_switch(&resp, label, *on);
    if let Some(d) = description {
        a11y::describe(&resp, d);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let mut y = rect.center().y - text_h / 2.0;
        let lh = label_g.size().y;
        painter.galley(pos2(rect.left(), y), label_g, p.t1);
        y += lh;
        if let Some(g) = desc_g {
            painter.galley(pos2(rect.left(), y), g, p.t3);
        }
        let track = Rect::from_center_size(
            pos2(rect.right() - control::TOGGLE_W / 2.0, rect.center().y),
            vec2(control::TOGGLE_W, control::TOGGLE_H),
        );
        paint_track(ui, &resp, track, *on);
        if resp.has_focus() {
            focus_ring(
                ui.painter(),
                track,
                CornerRadius::same(radius::PILL),
                &theme,
            );
        }
    }
    resp
}

/// Paint a switch in `track` for the control `resp`: the track goes from `o3` to the
/// accent fill and the knob from `knob` to `on_fill` (220 ms, linear) while it travels 12
/// (220 ms, `ease_out`).
pub(super) fn paint_track(ui: &Ui, resp: &Response, track: Rect, on: bool) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let painter = ui.painter();
    let colour = ui.ctx().animate_bool_with_time(
        resp.id.with("switch.fill"),
        on,
        theme.motion.fade(Dur::Base),
    );
    // `ease_out` both ways: the knob decelerates into its place whichever way it goes.
    let travel = glide(
        ui.ctx(),
        resp.id.with("switch.knob"),
        f32::from(u8::from(on)),
        theme.motion.movement(Dur::Base),
    );
    let corner = CornerRadius::same(radius::PILL);
    painter.rect_filled(track, corner, mix(p.o3, p.fill, colour));
    let r = control::TOGGLE_KNOB / 2.0;
    let inset = (control::TOGGLE_H - control::TOGGLE_KNOB) / 2.0;
    let x = track.left() + inset + r + control::TOGGLE_TRAVEL * travel;
    painter.circle_filled(pos2(x, track.center().y), r, mix(p.knob, p.on_fill, colour));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_is_32_by_20() {
        let r = crate::widgets::test_frame(&Default::default(), |ui| {
            let mut on = true;
            switch(ui, &mut on, "Show playtime under covers").rect
        });
        assert_eq!(r.size(), vec2(control::TOGGLE_W, control::TOGGLE_H));
    }

    #[test]
    fn the_knob_travels_twelve() {
        assert_eq!(
            control::TOGGLE_W - control::TOGGLE_KNOB - (control::TOGGLE_H - control::TOGGLE_KNOB),
            control::TOGGLE_TRAVEL
        );
    }
}
