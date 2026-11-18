//! The end column's top: the ANSWER BAR while a call rings (12 in, radius 12, o2 with a
//! 1-point line2, padding 10 · 12: the caller's avatar 36 with the ping ring, the name over
//! "is calling you", Decline and Answer at 34; Answer has the keyboard, Enter answers), then
//! the conversation header at 52 (22 in: avatar 36, the name 15 semibold over its status, an × that
//! goes back to the game).
use egui::{Align2, CornerRadius, Id, Key, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use zaun_core::chat::overlay::FromOverlay;
use zaun_core::chat::overlay::snapshot::OverlayRing;
use zaun_ui::theme::{Type, icons};

use super::button;
use super::layout::Row;
use crate::model::{Focus, Model};
use crate::paint::{avatar, icon, text};
use crate::words;

pub const BAR: f32 = 12.0 + 56.0;
pub const HEADER: f32 = 52.0;

pub fn answer_bar(ui: &mut Ui, m: &mut Model, ring: &OverlayRing, column: Rect, now: f64) {
    let look = m.look;
    let rtl = words::rtl();
    let rect = Rect::from_min_size(
        column.min + vec2(12.0, 12.0),
        vec2(column.width() - 24.0, 56.0),
    );
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(12), look.o2);
    p.rect_stroke(
        rect,
        CornerRadius::same(12),
        Stroke::new(1.0, look.line2),
        StrokeKind::Inside,
    );
    let row = Row::new(rect.shrink2(vec2(12.0, 10.0)), rtl);
    let av = row.span(0.0, 36.0, rect.top() + 10.0, 36.0);
    let t = (!m.reduced()).then(|| (now % 1.5 / 1.5) as f32);
    avatar::ping(ui.painter(), av.center(), 18.0, t, &look);
    avatar::avatar(ui.painter(), av, &ring.who, false, &m.pictures, &look, 1.0);
    let tx = row.x(36.0 + 10.0);
    text::line(
        ui,
        Type::BodyBold,
        &ring.who.name,
        look.t1,
        pos2(tx, av.top() + 1.0),
        300.0,
        rtl,
    );
    text::line(
        ui,
        Type::Caption,
        words::text("chat.details.calling_you"),
        look.t3,
        pos2(tx, av.top() + 19.0),
        300.0,
        rtl,
    );
    let answer_w = 34.0 + 70.0;
    let answer = row.end_span(0.0, answer_w, rect.center().y - 17.0, 34.0);
    let decline_g = text::galley(
        ui,
        Type::BodyBold,
        words::text("chat.details.decline"),
        look.t1,
        140.0,
        1,
    );
    let decline_w = decline_g.size().x + 24.0;
    let decline = row.end_span(answer_w + 8.0, decline_w, rect.center().y - 17.0, 34.0);
    let d = button::plate(
        ui,
        Id::unique("ovl-decline"),
        decline,
        8,
        &look,
        look.o2,
        "",
    );
    text::put(ui, &decline_g, decline.center(), Align2::CENTER_CENTER);
    let a = button::plate(
        ui,
        Id::unique("ovl-answer"),
        answer,
        8,
        &look,
        look.fill,
        "",
    );
    let ag = text::galley(
        ui,
        Type::BodyBold,
        words::text("chat.details.answer"),
        look.on_fill,
        120.0,
        1,
    );
    let inner_w = 16.0 + 8.0 + ag.size().x;
    let start = answer.center().x - inner_w / 2.0;
    let (glyph_x, text_x) = if rtl {
        (start + inner_w - 8.0, start)
    } else {
        (start + 8.0, start + 24.0)
    };
    icon::icon(
        ui.painter(),
        pos2(glyph_x, answer.center().y),
        icons::PHONE,
        15.0,
        look.on_fill,
    );
    text::put(
        ui,
        &ag,
        pos2(text_x, answer.center().y),
        Align2::LEFT_CENTER,
    );
    if m.focus == Some(Focus::Answer) {
        a.request_focus();
        m.focus = None;
    }
    let enter = a.has_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if a.clicked() || enter {
        m.said.push(FromOverlay::Answer);
        m.focus = Some(Focus::Composer);
    } else if d.clicked() {
        m.said.push(FromOverlay::Decline);
    }
}

pub fn header(ui: &mut Ui, m: &mut Model, rect: Rect) {
    let look = m.look;
    let rtl = words::rtl();
    let row = Row::new(rect, rtl);
    let snap = std::sync::Arc::clone(&m.snap);
    let Some(open) = &snap.open else {
        return;
    };
    let mid = rect.center().y;
    let av = row.span(22.0, 36.0, mid - 18.0, 36.0);
    avatar::avatar(
        ui.painter(),
        av,
        &open.who,
        open.chat.is_group(),
        &m.pictures,
        &look,
        1.0,
    );
    if let Some(s) = open.status {
        avatar::status(ui.painter(), av, s, &look, look.bg, rtl);
    }
    let tx = row.x(22.0 + 36.0 + 12.0);
    let width = rect.width() - 22.0 - 36.0 - 12.0 - 50.0;
    text::line(
        ui,
        Type::Heading,
        &open.who.name,
        look.t1,
        pos2(tx, mid - 18.0),
        width,
        rtl,
    );
    text::line(
        ui,
        Type::Caption,
        &m.labels.header,
        look.t3,
        pos2(tx, mid + 2.0),
        width,
        rtl,
    );
    let close = row.end_span(10.0, 30.0, mid - 15.0, 30.0);
    let r = button::plate(
        ui,
        Id::unique("ovl-close"),
        close,
        8,
        &look,
        egui::Color32::TRANSPARENT,
        words::text("overlay.back_short"),
    );
    icon::icon(ui.painter(), close.center(), icons::CLOSE, 14.0, look.t2);
    if r.clicked() {
        m.said.push(FromOverlay::Closed);
    }
}
