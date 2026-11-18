//! The details panel (chat.html section 8 DETAILS, script section 26 `detailsHTML`): 312
//! wide (mod.rs places it and slides it), padded 12 · 24 · 48; a friend's or a group's.
//! Owner: the details agent.
//!
//! Friend: × · avatar 72 with status 16 · name in display 30 · status line · about · Call
//! and Share game · Playing now · Also plays · Your connection (Friends since, Security,
//! Mute notifications) · Block and Remove friend in coral. Group: rounded-square avatar ·
//! name · members and online · Join or Start voice and Invite · Members (a click offers
//! Message · Profile · Block… · Mute notifications) · This group (Mute) · Leave group.
//! Rows whose feature the engine lacks are not drawn.

use easyplay_core::model::{
    ChatGroup, ChatRef, ChatSnapshot, Friend, GroupMember, Playing, Presence,
};
use easyplay_core::service::ChatCommand;
use egui::{
    Align, Align2, Color32, CornerRadius, Id, Layout, Popup, PopupCloseBehavior, Rect, RectAlign,
    Sense, Ui, UiBuilder, pos2, vec2,
};

use super::avatar::{self, Shape};
use super::{Env, paint};
use crate::app::UiState;
use crate::bridge::{Command, UiCommand};
use crate::route::Route;
use crate::strings::chat::details as s;
use crate::theme::palette::{Palette, fade, mix};
use crate::theme::tokens::{control, layout, radius};
use crate::theme::{self, Type, icons};
use crate::vm::chat::ChatDialog;
use crate::vm::dialogs::{AfterDiscard, Dialog};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::{Button, Variant};
use crate::widgets::context_menu;
use crate::widgets::icon_button::IconButton;
use crate::widgets::toggle::switch;

/// The panel's measures (`.dt`).
mod m {
    /// Padding: top, sides, bottom.
    pub const PAD: [f32; 3] = [12.0, 24.0, 48.0];
    /// Under the ×, and how far it hangs out to the start.
    pub const CLOSE: [f32; 2] = [8.0, 8.0];
    /// The avatar.
    pub const AVATAR: f32 = 72.0;
    /// Above the name; its size and line.
    pub const NAME: [f32; 3] = [14.0, 30.0, 30.0];
    /// Above the status line; its line.
    pub const STATUS: [f32; 2] = [5.0, 18.0];
    /// Above the about line.
    pub const ABOUT: f32 = 12.0;
    /// Above the actions; between them.
    pub const ACTS: [f32; 2] = [16.0, 8.0];
    /// Above a heading.
    pub const HEADING: f32 = 26.0;
    /// Under a heading.
    pub const HEADING_BELOW: f32 = 8.0;
    /// The playing-now poster, the gap after it, the row's padding, its radius.
    pub const NOW: [f32; 4] = [56.0, 12.0, 8.0, 10.0];
    /// Above Watch.
    pub const WATCH: f32 = 10.0;
    /// Also plays: posters, gap, how many.
    pub const SEEN: [f32; 2] = [60.0, 10.0];
    /// See [`SEEN`].
    pub const SEEN_MAX: usize = 4;
    /// A fact's label column and a switch row's least height.
    pub const ROW: [f32; 2] = [104.0, 34.0];
    /// Above and below a fact's line.
    pub const FACT_PAD: f32 = 6.5;
    /// Above the coral buttons, how far they hang out, between them.
    pub const BAD: [f32; 3] = [22.0, 10.0, 2.0];
    /// A member: avatar, row height, gap, radius.
    pub const MEMBER: [f32; 4] = [28.0, 42.0, 10.0, 8.0];
}

/// The details of `chat`, filling `ui` (the 312 panel mod.rs places).
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    egui::ScrollArea::vertical()
        .id_salt(("chat.details", chat))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let [top, side, bottom] = m::PAD;
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: side as i8,
                    right: side as i8,
                    top: top as i8,
                    bottom: bottom as i8,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    ui.set_width(ui.available_width());
                    close_button(ui, state);
                    match chat {
                        ChatRef::Direct(id) => {
                            if let Some(f) = snap.friend(id) {
                                friend(ui, state, env, snap, f, &p);
                            }
                        }
                        ChatRef::Group(id) => {
                            if let Some(g) = snap.group(id) {
                                group(ui, state, env, snap, g, &p);
                            }
                        }
                    }
                });
        });
}

fn rtl() -> bool {
    crate::i18n::dir() == crate::i18n::Dir::Rtl
}

/// The × at the panel's start.
fn close_button(ui: &mut Ui, state: &mut UiState) {
    let [below, out] = m::CLOSE;
    let w = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(w, control::BUTTON), Sense::hover());
    let x = if rtl() {
        row.right() + out - control::BUTTON
    } else {
        row.left() - out
    };
    let r = Rect::from_min_size(pos2(x, row.top()), vec2(control::BUTTON, control::BUTTON));
    if paint::place(ui, r, IconButton::new(icons::CLOSE, s::CLOSE_DETAILS())).clicked() {
        state.chat.details = Some(false);
    }
    ui.add_space(below);
}

/// The avatar 72 at the column's start, with its status 16 when `presence` says so.
fn big_avatar(
    ui: &mut Ui,
    name: &str,
    paper: Option<u8>,
    shape: Shape,
    picture: Option<&easyplay_core::model::Picture>,
    presence: Option<Presence>,
    p: &Palette,
) {
    let w = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(w, m::AVATAR), Sense::hover());
    let x = if rtl() {
        row.right() - m::AVATAR
    } else {
        row.left()
    };
    let r = Rect::from_min_size(pos2(x, row.top()), vec2(m::AVATAR, m::AVATAR));
    avatar::pictured(ui.painter(), r, name, paper, shape, picture);
    if let Some(pr) = presence {
        avatar::status(ui.painter(), r, pr, p.bg, true);
    }
}

/// The name in display 30 / 30, balanced, 14 above.
fn name_line(ui: &mut Ui, name: &str, p: &Palette) {
    let [above, size, line] = m::NAME;
    ui.add_space(above);
    let w = ui.available_width();
    let mut job = egui::text::LayoutJob::single_section(
        name.to_owned(),
        egui::TextFormat {
            font_id: theme::typography::display_font(size),
            color: p.t1,
            line_height: Some(line),
            ..Default::default()
        },
    );
    job.wrap.max_width = w;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    let x = if rtl() {
        r.right() - g.size().x
    } else {
        r.left()
    };
    ui.painter().galley(pos2(x, r.top()), g, p.t1);
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, name);
}

/// A line in `style` at the column's start (to its end right to left), wrapped.
fn line(ui: &mut Ui, text: &str, style: Type, color: Color32, line_h: Option<f32>) {
    let w = ui.available_width();
    let mut job = style.job(text, color, w, usize::MAX);
    if let Some(h) = line_h {
        for sct in &mut job.sections {
            sct.format.line_height = Some(h);
        }
    }
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    let x = if rtl() {
        r.right() - g.size().x
    } else {
        r.left()
    };
    ui.painter().galley(pos2(x, r.top()), g, color);
    crate::a11y::item(&resp, egui::Role::Label, text);
}

/// A heading (`.dt .h`): 26 above, 8 below.
fn heading(ui: &mut Ui, text: &str, p: &Palette) {
    ui.add_space(m::HEADING);
    let g = Type::Heading.line(ui, text, p.t1);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
    let x = if rtl() {
        r.right() - g.size().x
    } else {
        r.left()
    };
    ui.painter().galley(pos2(x, r.top()), g, p.t1);
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, text);
    ui.add_space(m::HEADING_BELOW);
}

/// Two box buttons side by side, each half the column (`.dt-acts`); `None` leaves one out.
/// A `Some(reason)` disables a button with that tooltip.
fn acts(
    ui: &mut Ui,
    a: Option<(&str, theme::Icon, Option<&str>)>,
    b: Option<(&str, theme::Icon, Option<&str>)>,
) -> (bool, bool) {
    let n = usize::from(a.is_some()) + usize::from(b.is_some());
    if n == 0 {
        return (false, false);
    }
    let [above, gap] = m::ACTS;
    ui.add_space(above);
    let w = ui.available_width();
    let bw = (w - gap * (n as f32 - 1.0)) / n as f32;
    let (row, _) = ui.allocate_exact_size(vec2(w, control::BUTTON), Sense::hover());
    let mut out = [false, false];
    let mut i = 0;
    for (k, spec) in [a, b].into_iter().enumerate() {
        let Some((label, icon, off)) = spec else {
            continue;
        };
        let from = i as f32 * (bw + gap);
        let x = if rtl() {
            row.right() - from - bw
        } else {
            row.left() + from
        };
        let r = Rect::from_min_size(pos2(x, row.top()), vec2(bw, control::BUTTON));
        let button = Button::new(label).icon(icon).min_width(bw).disabled(off);
        out[k] = paint::place(ui, r, button).clicked() && off.is_none();
        i += 1;
    }
    (out[0], out[1])
}

/// A fact row (`.dt .row`): a 104 label column, at least 34 high; with `door` the whole
/// row is a button with a caret at its end.
fn fact(ui: &mut Ui, label: &str, value: &str, door: bool, p: &Palette) -> egui::Response {
    let th = theme::theme(ui.ctx());
    let label_w = m::ROW[0];
    let w = ui.available_width();
    let caret = if door { 13.0 } else { 0.0 };
    let k = Type::Body.galley(ui, label, p.t3, label_w - 8.0, 2);
    let v = Type::Body.galley(
        ui,
        value,
        p.t1,
        (w - label_w - caret - 8.0).max(1.0),
        usize::MAX,
    );
    // A fact is its line and 6.5 above and below (`.dt .row`); only a switch row is 34.
    let h = k.size().y.max(v.size().y) + m::FACT_PAD * 2.0;
    let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let id = ui.make_persistent_id(("chat.details.fact", label));
    let resp = ui.interact(r, id, if door { Sense::click() } else { Sense::hover() });
    let over = r.expand2(vec2(layout::ROW_OVERHANG, 0.0));
    let corner = CornerRadius::same(radius::ROW);
    let hover = if door {
        crate::widgets::hover_t(&resp, &th)
    } else {
        0.0
    };
    if door {
        crate::a11y::button(&resp, &crate::strings::chat::spoken(&[label, value]));
        ui.painter().rect_filled(over, corner, fade(p.o1, hover));
    } else {
        crate::a11y::item(
            &resp,
            egui::Role::Label,
            &crate::strings::chat::spoken(&[label, value]),
        );
    }
    let cy = r.center().y;
    let (kx, vx, cx) = if rtl() {
        (
            r.right() - k.size().x,
            r.right() - label_w - v.size().x,
            r.left() + caret / 2.0,
        )
    } else {
        (r.left(), r.left() + label_w, r.right() - caret / 2.0)
    };
    ui.painter()
        .galley(pos2(kx, cy - k.size().y / 2.0), k, p.t3);
    ui.painter()
        .galley(pos2(vx, cy - v.size().y / 2.0), v, p.t1);
    if door {
        let glyph = if rtl() {
            icons::CARET_LEFT
        } else {
            icons::CARET_RIGHT
        };
        ui.painter().text(
            pos2(cx, cy),
            Align2::CENTER_CENTER,
            glyph.text(false),
            icons::font(13.0),
            mix(p.t3, p.t1, hover),
        );
        if resp.has_focus() {
            crate::widgets::focus_ring(ui.painter(), over, corner, &th);
        }
    }
    resp
}

/// A switch row (`.dt .f`): the label and the switch at the end, at least 34 high.
fn switch_row(ui: &mut Ui, label: &str, on: &mut bool, p: &Palette) -> bool {
    let w = ui.available_width();
    let h = m::ROW[1];
    let top = ui.cursor().min;
    let rect = Rect::from_min_size(top, vec2(w, h));
    let g = Type::Body.galley(ui, label, p.t1, w - 48.0, 1);
    let gx = if rtl() {
        rect.right() - g.size().x
    } else {
        rect.left()
    };
    ui.painter()
        .galley(pos2(gx, rect.center().y - g.size().y / 2.0), g, p.t1);
    let layout = if rtl() {
        Layout::left_to_right(Align::Center)
    } else {
        Layout::right_to_left(Align::Center)
    };
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(layout));
    let changed = switch(&mut child, on, label).changed();
    ui.allocate_rect(rect, Sense::hover());
    changed
}

/// The coral text buttons at the end (`.dt-bad`), each a `(label, variant)`; returns which
/// was clicked.
fn bad_buttons(ui: &mut Ui, items: &[(String, Variant)]) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    let [above, out, gap] = m::BAD;
    ui.add_space(above);
    let mut hit = None;
    for (i, (label, variant)) in items.iter().enumerate() {
        if i > 0 {
            ui.add_space(gap);
        }
        let w = ui.available_width();
        let (row, _) = ui.allocate_exact_size(vec2(w, control::BUTTON), Sense::hover());
        let bw = Type::BodyStrong
            .line(ui, label, Color32::PLACEHOLDER)
            .size()
            .x
            + control::BUTTON_PAD * 2.0;
        let x = if rtl() {
            row.right() + out - bw
        } else {
            row.left() - out
        };
        let r = Rect::from_min_size(pos2(x, row.top()), vec2(bw, control::BUTTON));
        if paint::place(ui, r, Button::new(label).variant(*variant)).clicked() {
            hit = Some(i);
        }
    }
    hit
}

/// "42 min", "1 h 5 min".
fn minutes(n: u32) -> String {
    crate::strings::fmt::play_time(u64::from(n) * 60_000)
}

/// The status words of someone around (or the game they play).
fn status_words(
    presence: Presence,
    playing: Option<&Playing>,
    presence_on: bool,
) -> (String, bool) {
    if !presence_on {
        return (String::new(), false);
    }
    if let Some(pl) = playing.filter(|_| presence != Presence::Offline) {
        return (s::playing(&pl.title), true);
    }
    let w = match presence {
        Presence::Online => s::ONLINE(),
        Presence::Away => s::AWAY(),
        Presence::Busy => s::BUSY(),
        Presence::Offline => s::OFFLINE(),
    };
    (w.to_owned(), false)
}

/// "January 2026".
fn month_year(t: easyplay_core::clock::Timestamp, offset_s: i32) -> String {
    use easyplay_core::stats::rollup::{civil_from_days, key_days, local_day};
    let (y, mo, _) = civil_from_days(key_days(local_day(t, (offset_s / 60) as i16)));
    let month = crate::strings::date::month_full((mo.clamp(1, 12) - 1) as usize);
    crate::strings::date::month_year(month, y)
}

/// A game of the library with this title (not hidden).
fn own_game(env: &Env<'_>, title: &str) -> Option<easyplay_core::ids::GameId> {
    env.bridge
        .library()
        .games
        .iter()
        .find(|g| !g.hidden && &*g.name == title)
        .map(|g| g.id)
}

// ── A friend ──────────────────────────────────────────────────────────────────────────

fn friend(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    f: &Friend,
    p: &Palette,
) {
    let fe = snap.features;
    let chat = ChatRef::Direct(f.id);
    let first = paint::first_name(&f.name).to_owned();
    let presence_on = fe.presence && !f.blocked;
    big_avatar(
        ui,
        &f.name,
        f.paper,
        Shape::Person,
        f.picture.as_ref(),
        presence_on.then_some(f.presence),
        p,
    );
    name_line(ui, &f.name, p);
    let (words, playing) = status_words(f.presence, f.playing.as_ref(), presence_on);
    if !words.is_empty() {
        ui.add_space(m::STATUS[0]);
        let color = if playing { p.accent_text } else { p.t2 };
        line(ui, &words, Type::Body, color, Some(m::STATUS[1]));
    }
    if !f.about.is_empty() && !f.blocked {
        ui.add_space(m::ABOUT);
        line(ui, &f.about, Type::Prose, p.t2, None);
    }
    let offline = fe.presence && f.presence == Presence::Offline;
    let offline_tip = s::offline_name(&first);
    let in_call = snap.live.call.is_some();
    if !f.blocked {
        let call_off = if offline {
            Some(offline_tip.as_str())
        } else if in_call {
            Some(s::IN_CALL())
        } else {
            None
        };
        let share_off = offline.then_some(offline_tip.as_str());
        let (call, share) = acts(
            ui,
            fe.calls.then_some((s::CALL(), icons::PHONE, call_off)),
            fe.share
                .then_some((s::SHARE_GAME(), icons::SHARE_GAME, share_off)),
        );
        if call {
            super::dialogs::call_flow(state, env, snap, chat);
        }
        if share {
            super::dialogs::share_flow(state, env, snap, chat);
        }
    }
    // Playing now.
    if presence_on
        && f.presence != Presence::Offline
        && let Some(pl) = &f.playing
    {
        heading(ui, s::PLAYING_NOW(), p);
        playing_now(ui, state, env, pl, p);
        let watching_them = snap.live.watch.as_ref().is_some_and(|w| w.chat == chat);
        if pl.live && fe.share && !watching_them {
            ui.add_space(m::WATCH);
            let w = ui.available_width();
            if ui.add(Button::primary(s::WATCH()).min_width(w)).clicked() {
                env.bridge.send(Command::Chat(ChatCommand::Watch(chat)));
            }
        }
    }
    // Also plays.
    if fe.presence && !f.blocked && !f.seen.is_empty() {
        heading(ui, s::ALSO_PLAYS(), p);
        also_plays(ui, &f.seen);
    }
    // Your connection.
    heading(ui, s::YOUR_CONNECTION(), p);
    if let Some(since) = f.since {
        fact(
            ui,
            s::FRIENDS_SINCE(),
            &month_year(since, env.utc_offset_s),
            false,
            p,
        );
    }
    if fe.verify {
        let v = if f.verified {
            s::VERIFIED()
        } else {
            s::NOT_VERIFIED()
        };
        if fact(ui, s::SECURITY(), v, true, p).clicked() {
            state.show_dialog(Dialog::Chat(ChatDialog::Verify(f.id)));
        }
    }
    if fe.mute && !f.blocked {
        let mut muted = f.muted;
        if switch_row(ui, s::MUTE_NOTIFICATIONS(), &mut muted, p) {
            env.bridge
                .send(Command::Chat(ChatCommand::SetMuted { chat, on: muted }));
        }
    }
    let mut items = Vec::new();
    if fe.block {
        if f.blocked {
            items.push((s::unblock_name(&first), Variant::Ghost));
        } else {
            items.push((s::block_name(&first), Variant::Warn));
        }
    }
    items.push((s::REMOVE_FRIEND().to_owned(), Variant::Warn));
    match bad_buttons(ui, &items) {
        Some(0) if fe.block => {
            if f.blocked {
                super::dialogs::set_blocked(state, env, f.id, false);
            } else {
                state.show_dialog(Dialog::Chat(ChatDialog::Block(f.id)));
            }
        }
        Some(_) => state.show_dialog(Dialog::Chat(ChatDialog::RemoveFriend(f.id))),
        None => {}
    }
}

/// The game a friend is in: its poster 56, title, time, and whether it is in your library
/// (the row then opens its page).
fn playing_now(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, pl: &Playing, p: &Palette) {
    let th = theme::theme(ui.ctx());
    let [pw, gap, pad, rad] = m::NOW;
    let own = own_game(env, &pl.title);
    let w = ui.available_width();
    let ph = pw * layout::COVER_ASPECT;
    let (r, _) = ui.allocate_exact_size(vec2(w, ph + pad * 2.0), Sense::hover());
    let row = r.expand2(vec2(pad, 0.0));
    let id = Id::unique(("chat.details.now", &pl.title));
    let resp = ui.interact(
        row,
        id,
        if own.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let corner = CornerRadius::same(rad as u8);
    let hover = if own.is_some() {
        crate::widgets::hover_t(&resp, &th)
    } else {
        0.0
    };
    ui.painter().rect_filled(row, corner, fade(p.o1, hover));
    let px = if rtl() { r.right() - pw } else { r.left() };
    let poster = Rect::from_min_size(pos2(px, r.top() + pad), vec2(pw, ph));
    crate::widgets::cover::paint(
        ui.painter(),
        poster,
        crate::widgets::cover::CoverSource::Placeholder,
        &pl.title,
        crate::widgets::cover::CoverLook::default(),
        &th,
    );
    let tw = (w - pw - gap).max(1.0);
    let time = if pl.live {
        s::sharing_live(&minutes(pl.minutes))
    } else {
        minutes(pl.minutes)
    };
    let lib = if own.is_some() {
        s::IN_LIBRARY()
    } else {
        s::NOT_IN_LIBRARY()
    };
    let a = Type::BodyStrong.galley(ui, &pl.title, p.t1, tw, 2);
    let b = Type::Caption.galley(ui, &time, p.t3, tw, 1);
    let c = Type::Caption.galley(ui, lib, p.t3, tw, 1);
    let h = a.size().y + b.size().y + c.size().y;
    let mut y = poster.center().y - h / 2.0;
    for (g, col) in [(a, p.t1), (b, p.t3), (c, p.t3)] {
        let x = if rtl() {
            poster.left() - gap - g.size().x
        } else {
            poster.right() + gap
        };
        let gh = g.size().y;
        ui.painter().galley(pos2(x, y), g, col);
        y += gh;
    }
    let spoken = crate::strings::chat::spoken(&[&pl.title, &time, lib]);
    if let Some(game) = own {
        crate::a11y::button(&resp, &spoken);
        let resp = crate::widgets::tooltip::attach(resp, s::OPEN_IN_LIBRARY());
        if resp.has_focus() {
            crate::widgets::focus_ring(ui.painter(), row, corner, &th);
        }
        if resp.clicked() {
            state.navigate(AfterDiscard::Route(Route::Game(game)));
        }
    } else {
        crate::a11y::item(&resp, egui::Role::Label, &spoken);
    }
}

/// Up to four 60 posters of games the app has seen them in.
fn also_plays(ui: &mut Ui, seen: &[String]) {
    let th = theme::theme(ui.ctx());
    let [pw, gap] = m::SEEN;
    let ph = pw * layout::COVER_ASPECT;
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, ph), Sense::hover());
    for (i, title) in seen.iter().take(m::SEEN_MAX).enumerate() {
        let from = i as f32 * (pw + gap);
        let x = if rtl() {
            r.right() - from - pw
        } else {
            r.left() + from
        };
        let pr = Rect::from_min_size(pos2(x, r.top()), vec2(pw, ph));
        avatar::small_poster(ui.painter(), pr, title, &th);
        let resp = ui.interact(pr, Id::unique(("chat.details.seen", i)), Sense::hover());
        crate::a11y::item(&resp, egui::Role::Image, title);
        crate::widgets::tooltip::attach(resp, title);
    }
}

// ── A group ───────────────────────────────────────────────────────────────────────────

/// Members in order: online first (online, away, busy, then the rest), then by name.
fn ranked(g: &ChatGroup, me: Presence) -> Vec<(GroupMember, Presence)> {
    let rank = |pr: Presence| match pr {
        Presence::Online => 0,
        Presence::Away => 1,
        Presence::Busy => 2,
        Presence::Offline => 3,
    };
    let mut all: Vec<(GroupMember, Presence)> = g
        .member_list
        .iter()
        .map(|m| (m.clone(), if m.me { me } else { m.presence }))
        .collect();
    all.sort_by(|(a, pa), (b, pb)| {
        rank(*pa)
            .cmp(&rank(*pb))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    all
}

fn group(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    g: &ChatGroup,
    p: &Palette,
) {
    let fe = snap.features;
    let chat = ChatRef::Group(g.id);
    let me = snap
        .profile
        .as_ref()
        .map_or(Presence::Online, |pr| avatar::my_presence(pr.status));
    let members = ranked(g, me);
    let pic = g.picture.as_ref();
    big_avatar(ui, &g.name, None, Shape::Group, pic, None, p);
    name_line(ui, &g.name, p);
    let total = g
        .members
        .unwrap_or(members.len() as u32)
        .max(members.len() as u32);
    let online = members
        .iter()
        .filter(|(m, pr)| *pr != Presence::Offline && !m.blocked)
        .count() as u32;
    ui.add_space(m::STATUS[0]);
    let words = if fe.presence {
        s::group_line(total, online)
    } else {
        crate::strings::chat::members(total)
    };
    line(ui, &words, Type::Body, p.t2, Some(m::STATUS[1]));
    let in_other_call = snap.live.call.as_ref().is_some_and(|c| c.chat != chat);
    let voice = if g.voice.is_empty() {
        s::START_VOICE()
    } else {
        s::JOIN_VOICE()
    };
    let in_this_call = snap.live.call.as_ref().is_some_and(|c| c.chat == chat);
    let (join, invite) = acts(
        ui,
        (fe.calls && fe.group_voice && !in_this_call).then_some((
            voice,
            icons::PHONE,
            in_other_call.then_some(s::IN_CALL()),
        )),
        Some((s::INVITE(), icons::USER_PLUS, None)),
    );
    if join {
        super::dialogs::call_flow(state, env, snap, chat);
    }
    if invite {
        state.show_dialog(Dialog::Chat(ChatDialog::InviteToGroup(g.id)));
    }
    heading(ui, s::MEMBERS(), p);
    for (mem, pr) in &members {
        member_row(ui, state, env, snap, g, mem, *pr, p);
    }
    heading(ui, s::THIS_GROUP(), p);
    // The group's picture, for its owner (pictures, ADR-0038).
    let row = crate::strings::chat::picture::GROUP_ROW();
    let change = crate::strings::chat::picture::CHANGE();
    if g.owner && fact(ui, row, change, true, p).clicked() {
        state.show_dialog(Dialog::Chat(ChatDialog::GroupPicture(g.id)));
    }
    if fe.mute {
        let mut muted = g.muted;
        if switch_row(ui, s::MUTE_NOTIFICATIONS(), &mut muted, p) {
            env.bridge
                .send(Command::Chat(ChatCommand::SetMuted { chat, on: muted }));
        }
    }
    if bad_buttons(ui, &[(s::LEAVE_GROUP().to_owned(), Variant::Warn)]).is_some() {
        state.show_dialog(Dialog::Chat(ChatDialog::LeaveGroup(g.id)));
    }
}

/// A member (`.mb`): avatar 28 with status, name over status words, "Owner" at the end. A
/// click on yourself opens your profile; on someone else, a menu.
#[allow(clippy::too_many_arguments)]
fn member_row(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    g: &ChatGroup,
    mem: &GroupMember,
    presence: Presence,
    p: &Palette,
) {
    let th = theme::theme(ui.ctx());
    let fe = snap.features;
    let [av, h, gap, rad] = m::MEMBER;
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let row = r.expand2(vec2(8.0, 0.0));
    let id = Id::unique(("chat.details.member", g.id, mem.id));
    let friend = mem.friend.and_then(|f| snap.friend(f));
    // A member you are not friends with has a menu only while blocking is there.
    let has_menu = mem.me || friend.is_some() || fe.block;
    let resp = ui.interact(
        row,
        id,
        if has_menu {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let menu_open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&resp));
    let hover = crate::widgets::hover_t(&resp, &th).max(if menu_open { 1.0 } else { 0.0 });
    let corner = CornerRadius::same(rad as u8);
    ui.painter().rect_filled(row, corner, fade(p.o1, hover));
    let paper = if mem.me {
        snap.profile.as_ref().and_then(|pr| pr.paper)
    } else {
        friend.and_then(|f| f.paper)
    };
    let ax = if rtl() { r.right() - av } else { r.left() };
    let ar = Rect::from_min_size(pos2(ax, r.center().y - av / 2.0), vec2(av, av));
    let pic = avatar::of_member(snap, mem);
    avatar::pictured(ui.painter(), ar, &mem.name, paper, Shape::Person, pic);
    let ring = mix(p.bg, p.o1, hover);
    if fe.presence && !mem.blocked {
        avatar::status(ui.painter(), ar, presence, ring, false);
    }
    let name = if mem.me { s::YOU() } else { mem.name.as_str() };
    let (words, playing) = if mem.me {
        let my = snap.profile.as_ref().map(|pr| pr.status);
        let w = match my {
            Some(easyplay_core::model::MyStatus::Invisible) => s::INVISIBLE().to_owned(),
            _ => status_words(presence, None, fe.presence).0,
        };
        (w, false)
    } else {
        status_words(presence, mem.playing.as_ref(), fe.presence && !mem.blocked)
    };
    let owner = mem.owner.then(|| Type::Caption.line(ui, s::OWNER(), p.t3));
    let ow = owner.as_ref().map_or(0.0, |o| o.size().x + gap);
    let tw = (w - av - gap - ow).max(1.0);
    let a = super::emoji::Line::new(ui, Type::BodyStrong, name, p.t1, tw);
    let b = Type::Caption.galley(
        ui,
        &words,
        if playing { p.accent_text } else { p.t3 },
        tw,
        1,
    );
    let th_h = a.size().y + if words.is_empty() { 0.0 } else { b.size().y };
    let ty = r.center().y - th_h / 2.0;
    let tx = |gw: f32| {
        if rtl() {
            ar.left() - gap - gw
        } else {
            ar.right() + gap
        }
    };
    let ah = a.size().y;
    let aw = a.size().x;
    a.paint(ui.painter(), pos2(tx(aw), ty), p.t1);
    if !words.is_empty() {
        ui.painter().galley(pos2(tx(b.size().x), ty + ah), b, p.t3);
    }
    if let Some(o) = owner {
        let ox = if rtl() {
            r.left()
        } else {
            r.right() - o.size().x
        };
        ui.painter()
            .galley(pos2(ox, r.center().y - o.size().y / 2.0), o, p.t3);
    }
    let spoken = if mem.owner {
        crate::strings::chat::spoken(&[name, &words, s::OWNER()])
    } else {
        crate::strings::chat::spoken(&[name, &words])
    };
    if has_menu {
        crate::a11y::button(&resp, &spoken);
    } else {
        crate::a11y::item(&resp, egui::Role::Label, &spoken);
    }
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), row, corner, &th);
    }
    if !has_menu {
        return;
    }
    if mem.me {
        if resp.clicked() {
            state.show_dialog(Dialog::Chat(ChatDialog::Profile));
        }
        return;
    }
    let popup = Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .align(RectAlign::BOTTOM_START);
    let first = paint::first_name(&mem.name).to_owned();
    let mut action = None;
    context_menu::popup(popup, |ui| {
        // Message · Profile · Mute notifications — Block (chat.html `member` menu).
        if let Some(f) = friend {
            if context_menu::icon_item(ui, icons::CHAT, s::MESSAGE(), None).clicked() {
                action = Some(MemberAction::Message(f.id));
            }
            if context_menu::icon_item(ui, icons::USER, s::PROFILE(), None).clicked() {
                action = Some(MemberAction::Profile(f.id));
            }
            if fe.mute
                && context_menu::icon_check_item(
                    ui,
                    icons::BELL_OFF,
                    s::MUTE_NOTIFICATIONS(),
                    f.muted,
                    true,
                )
                .clicked()
            {
                action = Some(MemberAction::Mute(f.id, !f.muted));
            }
        }
        if fe.block {
            if friend.is_some() {
                context_menu::separator(ui);
            }
            let blocked = friend.map_or(mem.blocked, |f| f.blocked);
            if blocked {
                if context_menu::icon_item(ui, icons::PROHIBIT, &s::unblock_name(&first), None)
                    .clicked()
                {
                    action = Some(MemberAction::Block(false));
                }
            } else if context_menu::icon_danger_item(
                ui,
                icons::PROHIBIT,
                &s::block_name(&first),
                None,
            )
            .clicked()
            {
                action = Some(MemberAction::Block(true));
            }
        }
    });
    match action {
        Some(MemberAction::Message(f)) => super::open_chat(state, env, ChatRef::Direct(f)),
        Some(MemberAction::Profile(f)) => {
            super::open_chat(state, env, ChatRef::Direct(f));
            state.chat.details = Some(true);
        }
        Some(MemberAction::Block(on)) => match friend {
            Some(f) if on => state.show_dialog(Dialog::Chat(ChatDialog::Block(f.id))),
            Some(f) => super::dialogs::set_blocked(state, env, f.id, false),
            None => {
                // Someone you are not friends with: their messages collapse in this group.
                let cmd = |on| {
                    UiCommand::Core(Command::Chat(ChatCommand::SetMemberBlocked {
                        group: g.id,
                        member: mem.id,
                        on,
                    }))
                };
                env.bridge.send(cmd(on));
                let (text, undo) = if on {
                    (s::blocked_toast(&first), Some(cmd(false)))
                } else {
                    (s::unblocked_toast(&first), None)
                };
                state.toast(text, ToastKind::Success, undo, env.mono);
            }
        },
        Some(MemberAction::Mute(f, on)) => env.bridge.send(Command::Chat(ChatCommand::SetMuted {
            chat: ChatRef::Direct(f),
            on,
        })),
        None => {}
    }
}

/// What a member's menu chose.
enum MemberAction {
    Message(i64),
    Profile(i64),
    Block(bool),
    Mute(i64, bool),
}
