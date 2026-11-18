//! The panel's start column (280, a hairline at its end): the top line (the mark, the
//! running game's title, the pin that opens "Show while playing"), then the chat list's
//! rows at 52 with 32 avatars, then the call when there is one (`open::call`).
use egui::{Align, Align2, CornerRadius, Id, Rect, Sense, Ui, pos2, vec2};
use zaun_core::chat::overlay::FromOverlay;
use zaun_ui::theme::Type;

use super::button;
use super::layout::Row;
use crate::model::Model;
use crate::paint::{avatar, icon, text};
use crate::words;

pub const WIDTH: f32 = 280.0;
const TOP: f32 = 52.0;
const ROW: f32 = 52.0;

pub fn top(ui: &mut Ui, m: &mut Model, rect: Rect) {
    let row = Row::new(rect, words::rtl());
    let look = m.look;
    let mid = rect.top() + TOP / 2.0;
    let mark = row.span(18.0, 18.0, mid - 9.0, 18.0);
    zaun_ui::widgets::brand::paint_mark(ui.painter(), mark, look.atext, look.bg);
    let snap = std::sync::Arc::clone(&m.snap);
    let game = snap.game.as_deref().unwrap_or_default();
    let width = WIDTH - 18.0 - 18.0 - 10.0 - 10.0 - 30.0 - 8.0;
    text::line(
        ui,
        Type::BodyStrong,
        game,
        look.t2,
        pos2(row.x(18.0 + 18.0 + 10.0), mid - 9.0),
        width,
        row.rtl,
    );
    let pin = row.end_span(10.0, 30.0, mid - 15.0, 30.0);
    let idle = if m.pins {
        look.o2
    } else {
        egui::Color32::TRANSPARENT
    };
    let r = button::plate(
        ui,
        Id::unique("ovl-pin"),
        pin,
        8,
        &look,
        idle,
        words::text("overlay.show_while_playing"),
    );
    icon::pin(
        ui.painter(),
        pin.center(),
        15.0,
        if m.pins { look.t1 } else { look.t2 },
    );
    if r.clicked() {
        m.pins = !m.pins;
        m.corner_menu = false;
    }
}

/// The chat rows inside `rect` (scrolls when there are more than fit).
pub fn list(ui: &mut Ui, m: &mut Model, rect: Rect) {
    let look = m.look;
    let rtl = words::rtl();
    let open = m.snap.open.as_ref().map(|o| o.chat);
    let mut picked = None;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 0.0))));
    egui::ScrollArea::vertical()
        .id_salt("ovl-list")
        .auto_shrink(false)
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let w = ui.available_width();
            for (i, c) in m.snap.chats.iter().enumerate() {
                let labels = m.labels.chats.get(i);
                let (rect, r) = ui.allocate_exact_size(vec2(w, ROW), Sense::click());
                let row = Row::new(rect, rtl);
                let current = open == Some(c.chat);
                let fill = if current {
                    look.o2
                } else if r.hovered() || r.has_focus() {
                    look.o1
                } else {
                    egui::Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
                let ring = if current {
                    look.bg.lerp_to_gamma(look.t1, 0.09)
                } else {
                    look.bg
                };
                let av = row.span(10.0, 32.0, rect.center().y - 16.0, 32.0);
                avatar::avatar(
                    ui.painter(),
                    av,
                    &c.who,
                    c.chat.is_group(),
                    &m.pictures,
                    &look,
                    1.0,
                );
                if let Some(s) = c.status {
                    avatar::status(ui.painter(), av, s, &look, ring, rtl);
                }
                let unread = c.unread > 0 && !c.muted;
                let time = labels.map_or("", |l| l.time.as_str());
                let tg = text::galley(ui, Type::Micro, time, look.t3, 80.0, 1);
                let meta_w = tg.size().x.max(18.0);
                let text_w = rect.width() - 10.0 - 32.0 - 12.0 - 8.0 - meta_w - 10.0;
                // The render (ovl-open) keeps unread rows in the quiet colours; the count says it.
                let name_t = Type::BodyStrong;
                let ng = text::galley(ui, name_t, &c.who.name, look.t1, text_w, 1);
                let preview = labels.map_or("", |l| l.preview.as_str());
                let pg = text::galley(ui, Type::Caption, preview, look.t3, text_w, 1);
                let tx = row.x(10.0 + 32.0 + 12.0);
                let block = 18.0 + 1.0 + 16.0;
                let y = rect.center().y - block / 2.0;
                text::put(ui, &ng, pos2(tx, y), row.start_anchor(Align::TOP));
                text::put(ui, &pg, pos2(tx, y + 19.0), row.start_anchor(Align::TOP));
                let ex = row.x(rect.width() - 10.0);
                if unread {
                    text::put(
                        ui,
                        &tg,
                        pos2(ex, rect.center().y - 2.5),
                        row.end_anchor(Align::BOTTOM),
                    );
                    let n = labels.map_or("", |l| l.count.as_str());
                    count(ui, m, pos2(ex, rect.center().y + 2.5), n, rtl);
                } else {
                    text::put(
                        ui,
                        &tg,
                        pos2(ex, rect.center().y),
                        row.end_anchor(Align::Center),
                    );
                }
                if r.clicked() && !current {
                    picked = Some(c.chat);
                }
            }
        });
    if let Some(chat) = picked {
        m.said.push(FromOverlay::OpenConversation { chat });
        m.focus = Some(crate::model::Focus::Composer);
    }
}

/// The unread count pill (18 high, the accent fill), its end at `end`.
fn count(ui: &Ui, m: &Model, end: egui::Pos2, n: &str, rtl: bool) {
    let look = &m.look;
    let g = text::galley(ui, Type::MicroStrong, n, look.on_fill, 60.0, 1);
    let w = (g.size().x + 10.0).max(18.0);
    let x = if rtl { end.x } else { end.x - w };
    let r = Rect::from_min_size(pos2(x, end.y), vec2(w, 18.0));
    ui.painter()
        .rect_filled(r, CornerRadius::same(9), look.fill);
    text::put(ui, &g, r.center(), Align2::CENTER_CENTER);
}
