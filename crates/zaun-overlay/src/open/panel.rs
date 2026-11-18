//! The open overlay (`chat-calls.html` §G OPEN): one flat dim (black 60 %, 140 linear in and
//! out), the panel centred 14 above the middle, width clamp(960, 54 % W, 1280) and height
//! clamp(600, 60 % H, 820), bg with a 1-point line2, radius 14, no shadow; in: alpha 140
//! linear and scale .98 → 1 (out: the mirror; reduced motion: alpha only). The start column
//! is 280 with a hairline at its end; the end column holds the answer bar, the header, the
//! transcript and the composer. The hint sits 16 under it.
use egui::emath::TSTransform;
use egui::{
    Area, Color32, Context, CornerRadius, Id, LayerId, Order, Rect, Stroke, StrokeKind, Ui, pos2,
    vec2,
};
use zaun_ui::theme::Type;

use super::{call, composer, head, hint, keys, pins, side, transcript};
use crate::model::Model;
use crate::motion::{self, FAST, REDUCED};
use crate::paint::text;
use crate::words;

/// The panel's size on a `screen` (points), never larger than the screen allows.
pub fn size(screen: Rect) -> egui::Vec2 {
    let w = (screen.width() * 0.54)
        .clamp(960.0, 1280.0)
        .min(screen.width() - 48.0);
    let h = (screen.height() * 0.60)
        .clamp(600.0, 820.0)
        .min(screen.height() - 96.0);
    vec2(w, h)
}

pub fn draw(ctx: &Context, m: &mut Model, now: f64) {
    let len = if m.reduced() { REDUCED } else { FAST };
    let t = m.open.value(now, len);
    if m.open.moving(now, len) {
        ctx.request_repaint();
    }
    if t <= 0.0 {
        m.pins = false;
        m.corner_menu = false;
        return;
    }
    if m.open.is_on() {
        keys::handle(ctx, m);
    }
    let screen = ctx.content_rect();
    ctx.layer_painter(LayerId::new(Order::Background, Id::unique("ovl-dim")))
        .rect_filled(screen, 0.0, Color32::from_black_alpha((153.0 * t) as u8));
    let centre = pos2(screen.center().x, screen.center().y - 14.0);
    let rect = Rect::from_center_size(centre, size(screen));
    let scale = if m.reduced() {
        1.0
    } else if m.open.is_on() {
        0.98 + 0.02 * motion::ease_out(t)
    } else {
        0.98 + 0.02 * (1.0 - motion::ease_in(1.0 - t))
    };
    let mut pin_anchor = None;
    let shown = Area::new(Id::unique("ovl-panel"))
        .order(Order::Middle)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            ui.set_min_size(rect.size());
            ui.set_clip_rect(rect);
            pin_anchor = Some(body(ui, m, rect, now));
        });
    let to = TSTransform::from_translation(centre.to_vec2())
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-centre.to_vec2());
    ctx.set_transform_layer(shown.response.layer_id, to);
    Area::new(Id::unique("ovl-hint"))
        .order(Order::Middle)
        .fixed_pos(pos2(screen.left(), rect.bottom() + 16.0))
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            hint::draw(ui, m, pos2(centre.x, rect.bottom() + 16.0));
        });
    if m.pins
        && m.open.is_on()
        && let Some(a) = pin_anchor
    {
        pins::draw(ctx, m, a);
    }
}

/// The panel's contents; returns where the popover hangs (under the pin).
fn body(ui: &mut Ui, m: &mut Model, rect: Rect, now: f64) -> egui::Pos2 {
    let look = m.look;
    // Borrowed for the frame: a new snapshot replaces the `Arc`, never this one.
    let snap = std::sync::Arc::clone(&m.snap);
    let rtl = words::rtl();
    ui.painter()
        .rect_filled(rect, CornerRadius::same(14), look.bg);
    let (start, end) = if rtl {
        (
            Rect::from_min_max(pos2(rect.right() - side::WIDTH, rect.top()), rect.max),
            Rect::from_min_max(rect.min, pos2(rect.right() - side::WIDTH, rect.bottom())),
        )
    } else {
        (
            Rect::from_min_max(rect.min, pos2(rect.left() + side::WIDTH, rect.bottom())),
            Rect::from_min_max(pos2(rect.left() + side::WIDTH, rect.top()), rect.max),
        )
    };
    let edge = if rtl { start.left() } else { start.right() };
    ui.painter()
        .vline(edge, start.y_range(), Stroke::new(1.0, look.line));
    // Start column: the top line, the chats, the call.
    let top = Rect::from_min_size(start.min, vec2(start.width(), 52.0));
    side::top(ui, m, top);
    let pin_at = if rtl {
        pos2(start.left() + 10.0, top.bottom())
    } else {
        pos2(start.right() - 40.0, top.bottom())
    };
    let call_h = if snap.call.is_some() {
        call::HEIGHT
    } else {
        0.0
    };
    let list = Rect::from_min_max(
        pos2(start.left(), top.bottom()),
        pos2(start.right(), start.bottom() - call_h),
    );
    side::list(ui, m, list);
    if let Some(c) = &snap.call {
        let r = Rect::from_min_max(pos2(start.left(), start.bottom() - call_h), start.max);
        call::draw(ui, m, c, r);
    }
    // End column: the answer bar, the header, the transcript, the composer.
    let mut y = end.top();
    if let Some(ring) = &snap.ringing {
        head::answer_bar(ui, m, ring, end, now);
        y += head::BAR;
    }
    let header = Rect::from_min_size(pos2(end.left(), y), vec2(end.width(), head::HEADER));
    head::header(ui, m, header);
    let comp = Rect::from_min_max(
        pos2(end.left() + 16.0, end.bottom() - 16.0 - composer::HEIGHT),
        pos2(end.right() - 16.0, end.bottom() - 16.0),
    );
    let msgs = Rect::from_min_max(
        pos2(end.left() + 24.0, header.bottom()),
        pos2(end.right() - 24.0, comp.top() - 10.0),
    );
    match &snap.open {
        Some(open) if open.messages.is_empty() => start_of_chat(ui, m, &open.who.name, msgs),
        Some(open) => transcript::draw(ui, m, open, msgs),
        None => {}
    }
    composer::draw(ui, m, comp);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(14),
        Stroke::new(1.0, look.line2),
        StrokeKind::Inside,
    );
    pin_at
}

/// An empty conversation: its name in the display face over the encryption sentence.
fn start_of_chat(ui: &Ui, m: &Model, name: &str, rect: Rect) {
    let look = &m.look;
    let row = super::layout::Row::new(rect, words::rtl());
    let x = row.x(48.0);
    let anchor = row.start_anchor(egui::Align::TOP);
    ui.painter().text(
        pos2(x, rect.top() + 26.0),
        anchor,
        name,
        zaun_ui::theme::typography::display_font(26.0),
        look.t1,
    );
    let body = text::galley(ui, Type::Prose, &m.labels.start, look.t3, 470.0, 4);
    text::put(ui, &body, pos2(x, rect.top() + 26.0 + 32.0), anchor);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_has_the_design_s_size_at_both_anchor_screens() {
        let at = |w: f32, h: f32| size(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)));
        let full_hd = at(1920.0, 1080.0);
        assert!((full_hd.x - 1036.8).abs() < 0.5 && (full_hd.y - 648.0).abs() < 0.5);
        assert_eq!(at(2560.0, 1440.0), vec2(1280.0, 820.0));
        // Never larger than the screen allows.
        assert!(at(800.0, 600.0).x <= 752.0);
    }
}
