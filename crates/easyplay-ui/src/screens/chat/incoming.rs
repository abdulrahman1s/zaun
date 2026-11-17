//! The incoming call card (top right, on every page) and the top bar's call / sharing pill
//! (chat.html section 8 INCOMING CALL, TOP BAR; script sections 23 `chatTop`, 25
//! `ringIn`). Owner: the details agent; the shell calls `card` once a frame on every page
//! and `top_bar_pill` in the top bar's right-to-left row, before the running game's pill.
//!
//! `card` is also where what waits on something else resolves, whatever page shows: a share
//! begins once the game started for it is running, the friends ticked in *New group* are
//! added once the group exists, and the stage shrinks when watching ends.

use easyplay_core::model::{Activity, ChatRef, ChatSnapshot};
use easyplay_core::service::ChatCommand;
use egui::{CornerRadius, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::Env;
use super::avatar::{self, Shape};
use super::paint::{self, Cc};
use crate::app::UiState;
use crate::bridge::Command;
use crate::route::Route;
use crate::strings::chat::details as s;
use crate::theme::palette::fade;
use crate::theme::tokens::{layout, radius, stroke};
use crate::theme::{self, Dur, Type, icons, motion};
use crate::vm::dialogs::AfterDiscard;
use crate::vm::toasts::ToastKind;

/// The card's measures (`.ring`).
mod m {
    /// Width.
    pub const W: f32 = 304.0;
    /// Padding.
    pub const PAD: f32 = 14.0;
    /// The caller's avatar.
    pub const AVATAR: f32 = 44.0;
    /// Between the grid's cells.
    pub const GAP: f32 = 12.0;
    /// The buttons' height.
    pub const BUTTON_H: f32 = 34.0;
    /// Between the buttons.
    pub const BUTTON_GAP: f32 = 8.0;
    /// Under the top bar.
    pub const BELOW_BAR: f32 = 10.0;
    /// The arrival's drop.
    pub const DROP: f32 = 12.0;
    /// The ring: 4 out from the avatar, 2 thick, grows to 1.45 every 1.5 s.
    pub const RING: [f32; 4] = [4.0, 2.0, 1.45, 1.5];
}

/// The pill's measures (`.callcap`).
mod pill {
    /// Height.
    pub const H: f32 = 30.0;
    /// Padding at its start and end.
    pub const PAD: [f32; 2] = [4.0, 3.0];
    /// Between its parts.
    pub const GAP: f32 = 7.0;
    /// The avatar.
    pub const AVATAR: f32 = 22.0;
    /// The round buttons.
    pub const CC: f32 = 24.0;
    /// The part that opens the chat is at most this wide.
    pub const OPEN_MAX: f32 = 220.0;
    /// Space to the running game's pill.
    pub const APART: f32 = 6.0;
}

/// The incoming call card, raised at the window's top right, on every page.
pub fn card(ctx: &egui::Context, state: &mut UiState, env: &mut Env<'_>) {
    let snap = env.bridge.chat().clone();
    resolve_waiting(state, env, &snap);
    let Some(ring) = snap.live.ringing.clone() else {
        return;
    };
    if !snap.features.calls {
        return;
    }
    let th = theme::theme(ctx);
    let p = th.palette.clone();
    let id = Id::unique(("chat.ring", ring.chat, ring.since.as_millis()));
    let (alpha, drop) = motion::arrive(ctx, id, th.motion, 0.0, Dur::Move, m::DROP);
    let screen = ctx.content_rect();
    let h = m::PAD * 2.0 + m::AVATAR + m::GAP + m::BUTTON_H;
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let x = if rtl {
        screen.left() + layout::TOP_BAR_END
    } else {
        screen.right() - layout::TOP_BAR_END - m::W
    };
    let top = screen.top() + layout::TOP_BAR + m::BELOW_BAR - drop;
    let rect = Rect::from_min_size(pos2(x, top), vec2(m::W, h));
    let (friend_paper, name) = match ring.chat {
        ChatRef::Direct(f) => (snap.friend(f).and_then(|f| f.paper), ring.name.clone()),
        ChatRef::Group(_) => (None, ring.name.clone()),
    };
    let mut answer = false;
    let mut decline = false;
    egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.multiply_opacity(alpha);
            let (r, resp) = ui.allocate_exact_size(rect.size(), Sense::hover());
            crate::a11y::labelled(
                ctx,
                resp.id,
                egui::Role::AlertDialog,
                &s::calling_spoken(&name),
            );
            let painter = ui.painter().clone();
            let corner = CornerRadius::same(radius::LG);
            painter.add(p.shade_pop.shadow().as_shape(r, corner));
            painter.rect_filled(r, corner, p.raised);
            painter.rect_stroke(
                r,
                corner,
                Stroke::new(stroke::BORDER, p.line),
                StrokeKind::Inside,
            );
            let inner = r.shrink(m::PAD);
            let av_x = if rtl {
                inner.right() - m::AVATAR
            } else {
                inner.left()
            };
            let av = Rect::from_min_size(pos2(av_x, inner.top()), vec2(m::AVATAR, m::AVATAR));
            ring_mark(&painter, av, &p);
            avatar::avatar(&painter, av, &name, friend_paper, Shape::Person);
            // Name over "is calling you".
            let tw = inner.width() - m::AVATAR - m::GAP;
            let n = super::emoji::Line::new(ui, Type::Heading, &name, p.t1, tw);
            let sub = Type::Body.galley(ui, s::CALLING_YOU(), p.t3, tw, 1);
            let th_h = n.size().y + sub.size().y;
            let ty = av.center().y - th_h / 2.0;
            let tx = |w: f32| {
                if rtl {
                    av.left() - m::GAP - w
                } else {
                    av.right() + m::GAP
                }
            };
            let nh = n.size().y;
            let nw = n.size().x;
            n.paint(&painter, pos2(tx(nw), ty), p.t1);
            painter.galley(pos2(tx(sub.size().x), ty + nh), sub, p.t3);
            // Decline (box) and Answer (filled, with the phone).
            let by = av.bottom() + m::GAP;
            let bw = (inner.width() - m::BUTTON_GAP) / 2.0;
            let first = Rect::from_min_size(pos2(inner.left(), by), vec2(bw, m::BUTTON_H));
            let second = first.translate(vec2(bw + m::BUTTON_GAP, 0.0));
            let (d_rect, a_rect) = if rtl {
                (second, first)
            } else {
                (first, second)
            };
            let d = paint::block_button(ui, d_rect, id.with("decline"), s::DECLINE(), None, false);
            let a = paint::block_button(
                ui,
                a_rect,
                id.with("answer"),
                s::ANSWER(),
                Some(icons::PHONE),
                true,
            );
            if crate::widgets::modal::opening(ctx, id.with("focus"))
                && crate::widgets::focus_visible(ctx)
            {
                a.request_focus();
            }
            answer = a.clicked();
            decline = d.clicked();
        });
    if answer {
        env.bridge.send(Command::Chat(ChatCommand::Answer));
        go_to_chat(state, env, ring.chat);
    } else if decline {
        env.bridge.send(Command::Chat(ChatCommand::Decline));
    }
}

/// The accent ring around the caller's avatar: it grows from 1 to 1.45 while it fades,
/// every 1.5 s; still under reduced motion or while the window is behind another.
fn ring_mark(painter: &egui::Painter, av: Rect, p: &theme::palette::Palette) {
    let [out, width, grow, period] = m::RING;
    let r0 = av.width() / 2.0 + out;
    let (scale, alpha) = if paint::moving(painter.ctx()) {
        painter.ctx().request_repaint();
        let t = ((painter.ctx().input(|i| i.time) / f64::from(period)).fract()) as f32;
        let e = motion::ease_out(t);
        (1.0 + (grow - 1.0) * e, 0.9 * (1.0 - e))
    } else {
        (1.0, 0.9)
    };
    painter.circle_stroke(
        av.center(),
        r0 * scale,
        Stroke::new(width, fade(p.accent_mark, alpha)),
    );
}

/// Show `chat` on the Chat page.
fn go_to_chat(state: &mut UiState, env: &mut Env<'_>, chat: ChatRef) {
    if state.nav.current() != Route::Chat {
        state.navigate(AfterDiscard::Route(Route::Chat));
    }
    super::open_chat(state, env, chat);
}

/// What waits on something else: a share for its game, a new group for its members, the
/// large stage for something to watch.
fn resolve_waiting(state: &mut UiState, env: &mut Env<'_>, snap: &ChatSnapshot) {
    if let Some(mut ps) = state.chat.pending_share {
        let activity = env
            .bridge
            .library()
            .games
            .iter()
            .find(|g| g.id == ps.game)
            .map(|g| g.activity);
        match activity {
            Some(Activity::Running { .. }) => {
                env.bridge
                    .send(Command::Chat(ChatCommand::StartShare(ps.chat)));
                state.chat.pending_share = None;
            }
            Some(Activity::Launching(_)) => {
                ps.started = true;
                state.chat.pending_share = Some(ps);
            }
            Some(Activity::Idle)
                if ps.started
                    || env.mono.saturating_duration_since(ps.asked)
                        > std::time::Duration::from_secs(crate::vm::chat::WAIT_SECS) =>
            {
                state.chat.pending_share = None;
            }
            None => state.chat.pending_share = None,
            _ => {}
        }
    }
    if let Some(pg) = &state.chat.pending_group
        && let Some(g) = snap
            .groups
            .iter()
            .find(|g| !pg.before.contains(&g.id) && g.name == pg.name)
    {
        let group = g.id;
        for &friend in &pg.friends {
            env.bridge
                .send(Command::Chat(ChatCommand::AddMember { group, friend }));
        }
        let name = pg.name.clone();
        state.chat.pending_group = None;
        state.toast(s::created(&name), ToastKind::Success, None, env.mono);
        if state.nav.current() == Route::Chat {
            super::open_chat(state, env, ChatRef::Group(group));
        }
    }
    if state.chat.pending_group.as_ref().is_some_and(|pg| {
        env.mono.saturating_duration_since(pg.asked)
            > std::time::Duration::from_secs(crate::vm::chat::WAIT_SECS)
    }) {
        state.chat.pending_group = None;
    }
    if snap.live.watch.is_none() {
        state.chat.stage_large = false;
    }
}

/// The conversation the pill is about: the call's, else the share's.
fn pill_chat(snap: &ChatSnapshot) -> Option<ChatRef> {
    snap.live
        .call
        .as_ref()
        .map(|c| c.chat)
        .or_else(|| snap.live.share.as_ref().map(|s| s.chat))
}

/// The top bar's pill while you are in a call or sharing, on every page but that
/// conversation's: laid out in the bar's right-to-left row.
pub fn top_bar_pill(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let snap = env.bridge.chat().clone();
    if !(snap.features.calls || snap.features.share) {
        return;
    }
    let Some(chat) = pill_chat(&snap) else {
        return;
    };
    let here = state.nav.current() == Route::Chat
        && state.chat.open.as_ref().is_some_and(|o| o.chat == chat);
    if here {
        return;
    }
    let th = theme::theme(ui.ctx());
    let p = th.palette.clone();
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let (name, paper, shape) = paint::face(&snap, chat);
    let who = paint::short_name(&snap, chat);
    let call = snap.live.call.as_ref().filter(|c| c.chat == chat);
    let sharing = snap.live.share.is_some();
    // The parts, in reading order: (width, what).
    enum Part {
        Avatar,
        Text(std::sync::Arc<egui::Galley>, egui::Color32),
        Live,
    }
    let mut parts: Vec<(f32, Part)> = Vec::new();
    let live_w = |label: &str| {
        let g = ui.painter().layout_no_wrap(
            label.to_owned(),
            egui::FontId::new(11.0, egui::FontFamily::Name("w600".into())),
            p.coral,
        );
        6.0 + 5.0 + g.size().x
    };
    if let Some(call) = call {
        parts.push((pill::AVATAR, Part::Avatar));
        let n = Type::BodyStrong.galley(ui, &who, p.t1, 120.0, 1);
        parts.push((n.size().x, Part::Text(n, p.t1)));
        if sharing {
            parts.push((live_w(s::SHARING()), Part::Live));
        }
        let clock = match call.since {
            Some(since) => {
                let ms = env.now.since(since).map_or(0, |d| d.as_millis() as u64);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(1_000 - ms % 1_000));
                paint::clock(ms)
            }
            None => s::RINGING().to_owned(),
        };
        let g = Type::BodyStrong.numeric_galley(ui, &clock, p.accent_text);
        parts.push((g.size().x, Part::Text(g, p.accent_text)));
    } else {
        parts.push((live_w(s::SHARING()), Part::Live));
        let n = Type::BodyStrong.galley(ui, &s::sharing_with(&who), p.t1, 160.0, 1);
        parts.push((n.size().x, Part::Text(n, p.t1)));
    }
    let open_lead = if call.is_some() {
        0.0
    } else {
        6.0 - pill::PAD[0]
    };
    let open_w = (open_lead
        + parts.iter().map(|(w, _)| *w).sum::<f32>()
        + pill::GAP * (parts.len().saturating_sub(1)) as f32)
        .min(pill::OPEN_MAX);
    let buttons = if call.is_some() { 2.0 } else { 1.0 };
    let w = pill::PAD[0] + open_w + (pill::GAP + pill::CC) * buttons + pill::PAD[1];
    if ui.available_width() < w + pill::APART {
        return;
    }
    let shown = motion::appear(
        ui.ctx(),
        Id::unique("chat.callcap"),
        th.motion.fade(Dur::Base),
    );
    ui.add_space(pill::APART);
    let (rect, _) = ui.allocate_exact_size(vec2(w, pill::H), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(radius::PILL), fade(p.o2, shown));
    // Positions from the pill's start edge.
    let at = |from: f32, width: f32| -> Rect {
        let x = if rtl {
            rect.right() - from - width
        } else {
            rect.left() + from
        };
        Rect::from_min_size(pos2(x, rect.top()), vec2(width, rect.height()))
    };
    let open_rect = at(pill::PAD[0] - 2.0, open_w + 4.0).shrink2(vec2(0.0, 3.0));
    let open = ui.interact(open_rect, Id::unique("chat.callcap.open"), Sense::click());
    crate::a11y::button(&open, &s::open_name(&who));
    let open = crate::widgets::tooltip::attach(open, &s::open_name(&who));
    if open.has_focus() {
        crate::widgets::focus_ring(&painter, open_rect, CornerRadius::same(12), &th);
    }
    let cy = rect.center().y;
    let mut x = pill::PAD[0] + open_lead;
    let clip = painter.with_clip_rect(at(pill::PAD[0], open_w).intersect(painter.clip_rect()));
    for (pw, part) in parts {
        let r = at(x, pw);
        match part {
            Part::Avatar => {
                let a = Rect::from_center_size(pos2(r.center().x, cy), vec2(pw, pw));
                avatar::avatar(&clip, a, &name, paper, shape);
            }
            Part::Text(g, c) => {
                clip.galley(pos2(r.left(), cy - g.size().y / 2.0), g, fade(c, shown));
            }
            Part::Live => {
                paint::live_mark(&clip, r.left(), cy, s::SHARING(), p.coral);
            }
        }
        x += pw + pill::GAP;
    }
    let mut x = pill::PAD[0] + open_w + pill::GAP;
    let mut next = || {
        let r = at(x, pill::CC);
        x += pill::CC + pill::GAP;
        Rect::from_center_size(r.center(), vec2(pill::CC, pill::CC))
    };
    if let Some(call) = call {
        let muted = call.muted;
        let mute = paint::call_button(
            ui,
            next(),
            Id::unique("chat.callcap.mute"),
            icons::MIC,
            Cc::Slash,
            muted,
            s::MUTE_MIC(),
            if muted { s::UNMUTE() } else { s::MUTE() },
        );
        let end = paint::call_button(
            ui,
            next(),
            Id::unique("chat.callcap.end"),
            icons::PHONE_END,
            Cc::End,
            false,
            s::LEAVE_CALL(),
            s::LEAVE_CALL(),
        );
        if mute.clicked() {
            env.bridge
                .send(Command::Chat(ChatCommand::CallMute(!muted)));
        }
        if end.clicked() {
            env.bridge.send(Command::Chat(ChatCommand::EndCall));
        }
    } else {
        let stop = paint::call_button(
            ui,
            next(),
            Id::unique("chat.callcap.stop"),
            icons::STOP,
            Cc::Plain,
            false,
            s::STOP_SHARING(),
            s::STOP_SHARING(),
        );
        if stop.clicked() {
            env.bridge.send(Command::Chat(ChatCommand::StopShare));
            state.toast(s::STOPPED_SHARING(), ToastKind::Info, None, env.mono);
        }
    }
    if open.clicked() {
        go_to_chat(state, env, chat);
    }
}
