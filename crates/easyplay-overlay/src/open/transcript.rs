//! The open conversation's transcript, with the window's rows and rhythm (`chat-calls.html`
//! §B): a GROUP is one person's run of messages (same author, same day, at most 6 minutes
//! apart, nothing between) or a run of events; 14 between groups, 0 inside one; a divider
//! (day, "New") has 22 above and 8 below. A head is 4 + an 18 name line + its text; a
//! continuation pads 2; an event is 28 (a 16 glyph centred in the 36 gutter, 13/20 words, the
//! time 8 after them). The text column starts 48 in; 24 side padding.
use easyplay_core::chat::overlay::snapshot::{BodyKind, CallLine, Conversation, OverlayMessage};
use easyplay_ui::theme::{Type, icons};
use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Rect, Sense, Stroke, Ui, pos2, vec2};

use std::ops::Range;

use super::layout::Row;
use crate::labels::MessageLabels;
use crate::model::Model;
use crate::paint::{avatar, icon, text};
use crate::words;

const GROUP_GAP: f32 = 14.0;
const DIVIDER_TOP: f32 = 22.0;
const DIVIDER_BOTTOM: f32 = 8.0;
const TEXT_X: f32 = 48.0;
const SAME_GROUP_MS: i64 = 6 * 60_000;
/// A line of message text (14/21).
const LINE: f32 = 21.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    Nothing,
    Divider,
    Message,
    Event,
}

pub fn draw(ui: &mut Ui, m: &Model, open: &Conversation, rect: Rect) {
    let rtl = words::rtl();
    let day_of = |at: i64| (at + i64::from(m.offset_s) * 1000).div_euclid(86_400_000);
    let first_new = open.messages.iter().position(|x| x.unread && !x.me);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    egui::ScrollArea::vertical()
        .id_salt(("ovl-transcript", open.chat))
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let w = ui.available_width();
            let col = w - 48.0;
            let mut prev = Prev::Nothing;
            let mut last: Option<&OverlayMessage> = None;
            for (i, msg) in open.messages.iter().enumerate() {
                let Some(l) = m.labels.messages.get(i) else {
                    break;
                };
                let new_day = last.is_none_or(|l| day_of(l.at) != day_of(msg.at));
                let is_new = first_new == Some(i);
                if (new_day && last.is_some()) || is_new {
                    divider(
                        ui,
                        m,
                        w,
                        rtl,
                        new_day.then_some(l.day.as_str()),
                        is_new,
                        prev,
                    );
                    prev = Prev::Divider;
                }
                let event = matches!(msg.kind, BodyKind::Notice | BodyKind::Call);
                if event {
                    if prev != Prev::Event && prev != Prev::Divider && prev != Prev::Nothing {
                        ui.add_space(GROUP_GAP);
                    }
                    event_row(ui, m, msg, l, w, rtl);
                    prev = Prev::Event;
                } else {
                    let same = prev == Prev::Message
                        && last.is_some_and(|l| {
                            l.me == msg.me
                                && l.author.name == msg.author.name
                                && msg.at - l.at <= SAME_GROUP_MS
                                && !matches!(l.kind, BodyKind::Notice | BodyKind::Call)
                        });
                    if !same && prev != Prev::Divider && prev != Prev::Nothing {
                        ui.add_space(GROUP_GAP);
                    }
                    message(ui, m, msg, l, !same, w, col, rtl);
                    prev = Prev::Message;
                }
                last = Some(msg);
            }
            ui.add_space(10.0);
        });
}

/// A day line ("Tuesday, 17 November 2026" and a hairline), a "New" line (an accent
/// hairline with "New" at its end), or both on one line.
fn divider(ui: &mut Ui, m: &Model, w: f32, rtl: bool, day: Option<&str>, new: bool, prev: Prev) {
    let look = &m.look;
    if prev != Prev::Nothing {
        ui.add_space(DIVIDER_TOP);
    }
    let (rect, _) = ui.allocate_exact_size(vec2(w, 16.0), Sense::hover());
    let row = Row::new(rect, rtl);
    let mut start = TEXT_X;
    if let Some(day) = day {
        let g = text::galley(ui, Type::Label, day, look.t3, w - TEXT_X, 1);
        text::put(
            ui,
            &g,
            pos2(row.x(TEXT_X), rect.top()),
            row.start_anchor(Align::TOP),
        );
        start += g.size().x + 12.0;
    }
    let mut end = 0.0;
    if new {
        let g = text::galley(
            ui,
            Type::MicroStrong,
            words::text("chat.conversation.new"),
            look.atext,
            80.0,
            1,
        );
        text::put(
            ui,
            &g,
            pos2(row.x(w), rect.center().y),
            row.end_anchor(Align::Center),
        );
        end = g.size().x + 12.0;
    }
    let colour = if new {
        look.amark.gamma_multiply(0.45)
    } else {
        look.line
    };
    let (a, b) = (row.x(start), row.x(w - end));
    ui.painter().hline(
        a.min(b)..=a.max(b),
        rect.center().y,
        Stroke::new(1.0, colour),
    );
    ui.add_space(DIVIDER_BOTTOM);
}

fn event_row(ui: &mut Ui, m: &Model, msg: &OverlayMessage, l: &MessageLabels, w: f32, rtl: bool) {
    let look = &m.look;
    let (rect, r) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    if r.hovered() {
        ui.painter()
            .rect_filled(rect.expand2(vec2(8.0, 0.0)), 8.0, look.o1);
    }
    let row = Row::new(rect, rtl);
    let bad = matches!(msg.call, Some(CallLine::Missed));
    let glyph = if msg.kind == BodyKind::Call {
        icons::PHONE
    } else {
        icons::INFO
    };
    let gc = row.span(10.0, 16.0, rect.center().y - 8.0, 16.0).center();
    icon::icon(
        ui.painter(),
        gc,
        glyph,
        15.0,
        if bad { look.coral } else { look.t3 },
    );
    let g = text::galley(ui, Type::Body, &l.body, look.t2, w - TEXT_X - 60.0, 1);
    let tr = text::put(
        ui,
        &g,
        pos2(row.x(TEXT_X), rect.center().y),
        row.start_anchor(Align::Center),
    );
    let tg = text::galley(ui, Type::Micro, &l.clock, look.t3, 60.0, 1);
    let tx = if rtl {
        tr.left() - 8.0
    } else {
        tr.right() + 8.0
    };
    text::put(
        ui,
        &tg,
        pos2(tx, rect.center().y + 1.0),
        row.start_anchor(Align::Center),
    );
}

#[allow(clippy::too_many_arguments)]
fn message(
    ui: &mut Ui,
    m: &Model,
    msg: &OverlayMessage,
    l: &MessageLabels,
    head: bool,
    w: f32,
    col: f32,
    rtl: bool,
) {
    let look = &m.look;
    let body = l.body.as_str();
    let colour = if msg.kind == BodyKind::Text {
        look.t1
    } else {
        look.t2
    };
    let g = if l.mentions.is_empty() || easyplay_ui::i18n::bidi::has_rtl(body) {
        text::galley(ui, Type::Prose, body, colour, col.min(680.0), 12)
    } else {
        ui.fonts_mut(|f| f.layout_job(mentions(body, &l.mentions, m, col.min(680.0))))
    };
    let lines = g.rows.len().max(1) as f32;
    let text_h = lines * LINE;
    let h = if head {
        4.0 + 18.0 + text_h + 2.0
    } else {
        2.0 + text_h + 2.0
    };
    let (rect, r) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if r.hovered() {
        ui.painter()
            .rect_filled(rect.expand2(vec2(8.0, 0.0)), 8.0, look.o1);
    }
    let row = Row::new(rect, rtl);
    let mut y = rect.top() + 2.0;
    if head {
        y = rect.top() + 4.0;
        let av = row.span(0.0, 36.0, y + 2.0, 36.0);
        avatar::avatar(ui.painter(), av, &msg.author, false, &m.pictures, look, 1.0);
        let name = if msg.me {
            msg.author.name.as_str()
        } else {
            words::first_name(&msg.author.name)
        };
        let ng = text::galley(ui, Type::BodyBold, name, look.t1, col * 0.6, 1);
        let nr = text::put(
            ui,
            &ng,
            pos2(row.x(TEXT_X), y),
            row.start_anchor(Align::TOP),
        );
        let tg = text::galley(ui, Type::Micro, &l.head_time, look.t3, 200.0, 1);
        let tx = if rtl {
            nr.left() - 8.0
        } else {
            nr.right() + 8.0
        };
        text::put(
            ui,
            &tg,
            pos2(tx, nr.bottom() - 2.0),
            row.start_anchor(Align::BOTTOM),
        );
        y += 18.0;
    }
    let anchor = row.start_anchor(Align::TOP);
    let at = pos2(row.x(TEXT_X), y + (LINE - 20.0) / 2.0);
    text::put(ui, &g, at, anchor);
}

/// Message text with its @mentions on the tint in accent text (left-to-right text only;
/// right-to-left lines go through the app's bidirectional layout without the highlight).
fn mentions(body: &str, at: &[Range<usize>], m: &Model, width: f32) -> LayoutJob {
    let look = &m.look;
    let plain = Type::Prose.format(look.t1, false);
    let mut marked = TextFormat {
        color: look.atext,
        background: look.tint,
        font_id: Type::BodyStrong.font_id(),
        ..plain.clone()
    };
    marked.font_id.size = plain.font_id.size;
    let mut job = LayoutJob::default();
    let mut from = 0;
    for r in at {
        job.append(&body[from..r.start], 0.0, plain.clone());
        job.append(&body[r.clone()], 0.0, marked.clone());
        from = r.end;
    }
    job.append(&body[from..], 0.0, plain);
    job.wrap.max_width = width;
    job
}
