//! Choices: the checkbox and radio marks, a quiet checkbox button, the whole-row choice of a
//! dialog ("pick") and the accent swatch.

use egui::accesskit::Role;
use egui::{
    Align, Align2, Color32, CornerRadius, Layout, Rect, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, UiBuilder, pos2, vec2,
};

use super::{focus_ring, pointer_t, press_t, tooltip};
use crate::a11y;
use crate::theme::motion::glide;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, radius, stroke};
use crate::theme::{self, Dur, Type, icons};

/// Paint a checkbox's box in `rect` (15 across): a quiet 1.5 pt ring, or the accent fill
/// with a check when `checked`.
fn paint_box(ui: &Ui, rect: Rect, checked: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let corner = CornerRadius::same(radius::XS);
    let painter = ui.painter();
    if checked {
        painter.rect_filled(rect, corner, p.fill);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            icons::CHECK.text(false),
            icons::font(control::CHECK_GLYPH),
            p.on_fill,
        );
    } else {
        painter.rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::FIELD_FOCUS, p.t3),
            StrokeKind::Inside,
        );
    }
}

/// Paint a radio's circle in `rect`: a quiet 1.5 pt ring that thickens to a 4.5 pt accent
/// ring as `chosen` (0–1) goes to 1.
fn paint_radio(ui: &Ui, rect: Rect, chosen: f32) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = stroke::FIELD_FOCUS + (stroke::RADIO_ON - stroke::FIELD_FOCUS) * chosen;
    let color = mix(p.t3, p.accent_mark, chosen.min(1.0));
    let r = rect.width() / 2.0 - w / 2.0;
    ui.painter()
        .circle_stroke(rect.center(), r, Stroke::new(w, color));
}

/// How far a radio has gone to chosen, animated over the hover time.
fn radio_t(ui: &Ui, id: egui::Id, chosen: bool) -> f32 {
    let theme = theme::theme(ui.ctx());
    // `ease_out` both ways (CSS `transition: box-shadow var(--fast) var(--out)`).
    glide(
        ui.ctx(),
        id.with("chosen"),
        f32::from(u8::from(chosen)),
        theme.motion.fade(Dur::Fast),
    )
}

/// Paint the 15 pt checkbox box at the cursor and return its rect. It takes no input: the
/// control around it does.
pub fn check_mark(ui: &mut Ui, checked: bool) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(control::CHECK, control::CHECK), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_box(ui, rect, checked);
    }
    rect
}

/// Paint the 15 pt radio circle at the cursor and return its rect. It takes no input.
pub fn radio_mark(ui: &mut Ui, chosen: bool) -> Rect {
    let (rect, resp) = ui.allocate_exact_size(vec2(control::CHECK, control::CHECK), Sense::hover());
    if ui.is_rect_visible(rect) {
        let t = radio_t(ui, resp.id, chosen);
        paint_radio(ui, rect, t);
    }
    rect
}

/// A quiet button with a checkbox before its label (the log bar's "Wrap", "Add several").
/// Clicking it flips `checked`.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let g = Type::BodyStrong.line(ui, label, Color32::PLACEHOLDER);
    let pad = control::BUTTON_PAD;
    let w = pad * 2.0 + control::CHECK + control::BUTTON_GAP + g.size().x;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, control::BUTTON), Sense::click());
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    let on = *checked;
    let enabled = ui.is_enabled();
    let name = label.to_owned();
    resp.widget_info(move || egui::WidgetInfo::selected(Role::CheckBox, enabled, on, &name));
    if ui.is_rect_visible(rect) {
        let hover = if enabled {
            pointer_t(&resp, &theme)
        } else {
            0.0
        };
        let pressed = if enabled { press_t(&resp, &theme) } else { 0.0 };
        let corner = CornerRadius::same(radius::MD);
        let fill = mix(fade(p.o2, hover), p.o3, pressed);
        let text = if enabled {
            mix(p.t2, p.t1, hover)
        } else {
            p.t4
        };
        let painter = ui.painter();
        painter.rect_filled(rect, corner, fill);
        if p.outlined() {
            painter.rect_stroke(
                rect,
                corner,
                Stroke::new(stroke::BORDER, p.line),
                StrokeKind::Inside,
            );
        }
        let cy = rect.center().y;
        let mark = Rect::from_min_size(
            pos2(rect.left() + pad, cy - control::CHECK / 2.0),
            vec2(control::CHECK, control::CHECK),
        );
        paint_box(ui, mark, on);
        let x = mark.right() + control::BUTTON_GAP;
        ui.painter().galley(pos2(x, cy - g.size().y / 2.0), g, text);
        if resp.has_focus() {
            focus_ring(ui.painter(), rect, corner, &theme);
        }
    }
    resp
}

/// Which mark a [`pick`] row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    /// One of several: a radio.
    Radio,
    /// Any of several: a checkbox.
    Check,
}

/// A whole-row choice in a dialog: at least 38 high, the mark at the left, then `content`
/// (a line of text, perhaps with a quiet line under it). Under the pointer a faint fill;
/// chosen, the tint. `name` is what assistive technology reads. Returns the row's response
/// (`clicked()` chooses it) and what `content` returned.
pub fn pick<R>(
    ui: &mut Ui,
    kind: PickKind,
    chosen: bool,
    name: &str,
    content: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let [pad_x, pad_y] = control::PICK_PAD;
    let full_w = ui.available_width();
    let top = ui.cursor().min;
    let id = ui.make_persistent_id(("pick", name));
    // The plate is painted under the content laid out after it: reserve its place.
    let plate = ui.painter().add(Shape::Noop);
    // The content is centred in the row: its height is known from the last pass.
    let known: Option<f32> = ui.ctx().data(|d| d.get_temp(id));
    let inner_min = control::PICK - pad_y * 2.0;
    let content_h = known.unwrap_or(inner_min);
    let inner_h = content_h.max(inner_min);
    let row_h = inner_h + pad_y * 2.0;
    let content_x = top.x + pad_x + control::CHECK + control::PICK_GAP;
    let content_w = (full_w - (content_x - top.x) - pad_x).max(1.0);
    let content_top = top.y + pad_y + (inner_h - content_h) / 2.0;
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(id.with("content"))
            .max_rect(Rect::from_min_size(
                pos2(content_x, content_top),
                vec2(content_w, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    child.spacing_mut().item_spacing.y = 0.0;
    let out = content(&mut child);
    let measured = child.min_rect().height();
    if known.is_none_or(|k| (k - measured).abs() > 0.5) {
        ui.ctx().data_mut(|d| d.insert_temp(id, measured));
        ui.ctx().request_discard("a pick row measured its content");
    }
    let rect = Rect::from_min_size(top, vec2(full_w, row_h));
    let resp = ui.interact(rect, id, Sense::click());
    ui.allocate_rect(rect, Sense::hover());
    let role = match kind {
        PickKind::Radio => Role::RadioButton,
        PickKind::Check => Role::CheckBox,
    };
    a11y::option(&resp, role, name, chosen);
    if ui.is_rect_visible(rect) {
        let hover = if ui.is_enabled() {
            pointer_t(&resp, &theme)
        } else {
            0.0
        };
        let corner = CornerRadius::same(radius::ROW);
        let fill = if chosen { p.tint } else { fade(p.o1, hover) };
        ui.painter()
            .set(plate, Shape::rect_filled(rect, corner, fill));
        let mark = Rect::from_center_size(
            pos2(top.x + pad_x + control::CHECK / 2.0, rect.center().y),
            vec2(control::CHECK, control::CHECK),
        );
        match kind {
            PickKind::Radio => {
                let t = radio_t(ui, id, chosen);
                paint_radio(ui, mark, t);
            }
            PickKind::Check => paint_box(ui, mark, chosen),
        }
        if resp.has_focus() {
            focus_ring(ui.painter(), rect, corner, &theme);
        }
    }
    (resp, out)
}

/// An accent swatch: a 22 pt disc of `color`; chosen, a 1.5 pt primary ring 2 away from it.
/// `name` is its accessible name and tooltip.
pub fn swatch(ui: &mut Ui, color: Color32, chosen: bool, name: &str) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let (rect, resp) =
        ui.allocate_exact_size(vec2(control::SWATCH, control::SWATCH), Sense::click());
    a11y::option(&resp, Role::RadioButton, name, chosen);
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(
            resp.id.with("chosen"),
            chosen,
            theme.motion.fade(Dur::Fast),
        );
        let painter = ui.painter();
        let r = control::SWATCH / 2.0;
        painter.circle_filled(rect.center(), r, color);
        if t > 0.0 {
            let w = stroke::FIELD_FOCUS;
            painter.circle_stroke(
                rect.center(),
                r + stroke::FOCUS_GAP + w / 2.0,
                Stroke::new(w, fade(p.t1, t)),
            );
        }
        if resp.has_focus() {
            // Outside the chosen ring, so both read at once.
            let ring = rect.expand(stroke::FOCUS_GAP + stroke::FIELD_FOCUS);
            focus_ring(painter, ring, CornerRadius::same(radius::PILL), &theme);
        }
    }
    tooltip::tooltip(resp, name, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl FnOnce(&mut Ui) -> Rect) -> Rect {
        crate::widgets::test_frame(&theme::Appearance::default(), f)
    }

    #[test]
    fn marks_are_15_across() {
        let r = run(|ui| check_mark(ui, true));
        assert_eq!(r.size(), vec2(control::CHECK, control::CHECK));
        let r = run(|ui| radio_mark(ui, false));
        assert_eq!(r.size(), vec2(control::CHECK, control::CHECK));
    }

    #[test]
    fn a_pick_row_is_at_least_38_high() {
        let r = run(|ui| {
            pick(ui, PickKind::Radio, true, "Restore a backup", |ui| {
                ui.label("Restore a backup");
            })
            .0
            .rect
        });
        assert!(r.height() >= control::PICK);
    }

    #[test]
    fn a_swatch_is_22_round() {
        let r = run(|ui| swatch(ui, Color32::RED, true, "Tide").rect);
        assert_eq!(r.size(), vec2(control::SWATCH, control::SWATCH));
    }
}
