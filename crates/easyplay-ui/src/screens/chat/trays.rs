//! The trays under a conversation's header (chat.html section 8 "trays",
//! `chat-calls.html` notes D–F): the call box (its stage or this tray), a group's voice you
//! have not joined, your own sharing, and the stage of the game you watch. Owner: callui.
//!
//! `show` is given the conversation's whole column and sets its own inset (24, 12 in a
//! narrow window) and the 10 under each tray; the stage centres in the column.

use easyplay_core::model::{ChatRef, ChatSnapshot};
use easyplay_core::service::ChatCommand;
use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::Env;
use super::avatar::{self, Shape as AvShape};
use super::paint::{self, Cc};
use crate::app::UiState;
use crate::bridge::Command;
use crate::strings::chat::calls as cs;
use crate::strings::chat::details as s;
use crate::theme::palette::{Palette, fade};
use crate::theme::tokens::stroke;
use crate::theme::{self, Dur, Type, icons, motion};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;

/// The trays' measures (`.call`, `.cc`, `.stage`, `.sb`).
mod m {
    /// A tray's height.
    pub const H: f32 = 60.0;
    /// Its radius.
    pub const RADIUS: u8 = 14;
    /// Under each tray.
    pub const BELOW: f32 = 10.0;
    /// The inset from the column's sides, and in a narrow window.
    pub const INSET: [f32; 2] = [24.0, 12.0];
    /// The page area is narrow below this width.
    pub const NARROW_BELOW: f32 = 760.0;
    /// Padding at the tray's start and end.
    pub const PAD: [f32; 2] = [14.0, 10.0];
    /// Between its parts.
    pub const GAP: f32 = 12.0;
    /// The avatars, 8 apart.
    pub const AVATAR: f32 = 32.0;
    /// See [`AVATAR`].
    pub const AVATAR_GAP: f32 = 8.0;
    /// The speaking ring: 2 thick, 2 out.
    pub const SPEAK: [f32; 2] = [2.0, 2.0];
    /// The round buttons, 6 apart.
    pub const CC: f32 = 36.0;
    /// See [`CC`].
    pub const CC_GAP: f32 = 6.0;
    /// Your shared game's cover.
    pub const COVER: f32 = 26.0;
    /// Paused: the cover keeps 20 % of its colour and 45 % of its light.
    pub const PAUSED: [f32; 2] = [0.2, 0.45];
}

/// The trays of `chat`, under its header.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
) {
    let f = snap.features;
    let narrow = ui.ctx().content_rect().width() < m::NARROW_BELOW;
    let inset = if narrow { m::INSET[1] } else { m::INSET[0] };
    let saved = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let call = snap.live.call.as_ref().filter(|c| c.chat == chat);
    if snap.live.call.is_none() {
        state.chat.call.call_over();
    }
    if f.calls {
        if let Some(call) = call {
            super::call_stage::show(ui, state, env, snap, call, inset);
        } else {
            super::call_stage::closing(ui, chat, inset);
            if let ChatRef::Group(g) = chat
                && let Some(group) = snap.group(g)
                && !group.voice.is_empty()
            {
                voice_tray(ui, state, env, snap, chat, inset);
            }
        }
    }
    if f.share {
        if let Some(sh) = snap.live.share.as_ref().filter(|s| s.chat == chat)
            && call.is_none()
        {
            share_tray(ui, state, env, snap, sh, inset);
        }
        let watched = snap.live.watch.as_ref().filter(|w| w.chat == chat);
        match watched {
            // A friend's game in this call takes the stage's big tile instead.
            Some(_) if call.is_some() => {}
            Some(w) => super::watch_stage::show(ui, state, env, snap, w, inset),
            None if snap.live.stream.ended == Some(chat) => {
                super::watch_stage::ended(ui, state, env, snap, chat, inset);
            }
            None => {}
        }
    }
    if snap.live.watch.is_none() {
        super::watch_stage::leave_full(ui, state, env);
    }
    if snap.live.stream.ended.is_none() {
        state.chat.call.ended_at = None;
    }
    super::call_stage::note_pass(ui, chat);
    ui.spacing_mut().item_spacing.y = saved;
}

/// The tray's plate (`.call`), faded in over 220; returns its rect and opacity.
fn plate(ui: &mut Ui, id: Id, inset: f32) -> (Rect, f32) {
    let th = theme::theme(ui.ctx());
    let p = &th.palette;
    let alpha = motion::appear(ui.ctx(), id, th.motion.fade(Dur::Base));
    let (row, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), m::H + m::BELOW), Sense::hover());
    let rect = Rect::from_min_size(
        pos2(row.left() + inset, row.top()),
        vec2((row.width() - inset * 2.0).max(1.0), m::H),
    );
    let corner = CornerRadius::same(m::RADIUS);
    ui.painter().rect_filled(rect, corner, fade(p.o1, alpha));
    if p.outlined() {
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::BORDER, p.line),
            StrokeKind::Inside,
        );
    }
    (rect, alpha)
}

/// Lays parts out from the tray's start edge, mirrored right to left.
struct Row {
    rect: Rect,
    rtl: bool,
    start: f32,
    end: f32,
}

impl Row {
    fn new(rect: Rect) -> Self {
        Self {
            rect,
            rtl: crate::i18n::dir() == crate::i18n::Dir::Rtl,
            start: m::PAD[0],
            end: m::PAD[1],
        }
    }

    fn rect_at(&self, from_start: f32, w: f32, h: f32) -> Rect {
        let x = if self.rtl {
            self.rect.right() - from_start - w
        } else {
            self.rect.left() + from_start
        };
        Rect::from_min_size(pos2(x, self.rect.center().y - h / 2.0), vec2(w, h))
    }

    /// The next `w` × `h` from the start.
    fn lead(&mut self, w: f32, h: f32, gap: f32) -> Rect {
        let r = self.rect_at(self.start, w, h);
        self.start += w + gap;
        r
    }

    /// The next `w` × `h` from the end.
    fn trail(&mut self, w: f32, h: f32, gap: f32) -> Rect {
        let from_start = self.rect.width() - self.end - w;
        let r = self.rect_at(from_start, w, h);
        self.end += w + gap;
        r
    }

    /// What is left between the two ends.
    fn middle(&self) -> Rect {
        let w = (self.rect.width() - self.start - self.end).max(1.0);
        self.rect_at(self.start, w, self.rect.height())
    }
}

/// Two lines (500 over 12 / 16 quiet) in `r`, cut to one line each.
fn two_lines(ui: &Ui, r: Rect, title: &str, sub: &str, p: &Palette, alpha: f32, rtl: bool) {
    let spoken = ui.interact(
        r,
        Id::unique(("chat.tray.text", r.min.y as i32)),
        Sense::hover(),
    );
    crate::a11y::item(
        &spoken,
        egui::Role::Label,
        &crate::strings::chat::spoken(&[title, sub]),
    );
    let a = Type::BodyStrong.galley(ui, title, p.t1, r.width(), 1);
    let b = Type::Caption.numeric_galley(ui, sub, p.t3);
    let b = if b.size().x > r.width() {
        Type::Caption.galley(ui, sub, p.t3, r.width(), 1)
    } else {
        b
    };
    let h = a.size().y + b.size().y;
    let y = r.center().y - h / 2.0;
    let x = |w: f32| if rtl { r.right() - w } else { r.left() };
    let painter = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    painter.galley(pos2(x(a.size().x), y), a.clone(), fade(p.t1, alpha));
    painter.galley(pos2(x(b.size().x), y + a.size().y), b, fade(p.t3, alpha));
}

/// Who someone in a call is: their name and avatar pair, by name.
fn person(snap: &ChatSnapshot, name: &str) -> (String, Option<u8>) {
    let paper = snap
        .friends
        .iter()
        .find(|f| f.name == name)
        .and_then(|f| f.paper);
    (name.to_owned(), paper)
}

/// An avatar of 32 in `r`, ringed in accent while `speaking` (the ring fades over 140).
fn speaker(
    ui: &Ui,
    r: Rect,
    name: &str,
    paper: Option<u8>,
    picture: Option<&easyplay_core::model::Picture>,
    speaking: bool,
    alpha: f32,
) {
    let th = theme::theme(ui.ctx());
    let t = ui.ctx().animate_bool_with_time(
        Id::unique(("chat.speaking", name)),
        speaking,
        th.motion.fade(Dur::Fast),
    );
    let [w, out] = m::SPEAK;
    if t > 0.0 {
        ui.painter().circle_stroke(
            r.center(),
            r.width() / 2.0 + out + w / 2.0,
            Stroke::new(w, fade(th.palette.accent_mark, t * alpha)),
        );
    }
    avatar::pictured(ui.painter(), r, name, paper, AvShape::Person, picture);
}

/// The call box made smaller (its plate is the box's): who, the clock, Make larger and the
/// round buttons.
pub fn call_tray_in(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    call: &easyplay_core::model::Call,
    rect: Rect,
    alpha: f32,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut row = Row::new(rect);
    // Avatars: the others, then you.
    let me = snap.profile.as_ref();
    let mut people: Vec<(String, Option<u8>, bool)> = call
        .with
        .iter()
        .map(|n| {
            let (n, paper) = person(snap, n);
            let speaking = call.speaking.contains(&n);
            (n, paper, speaking)
        })
        .collect();
    people.push((
        me.map(|p| p.name.clone()).unwrap_or_default(),
        me.and_then(|p| p.paper),
        false,
    ));
    let n = people.len();
    for (i, (name, paper, speaking)) in people.iter().enumerate() {
        let gap = if i + 1 == n { m::GAP } else { m::AVATAR_GAP };
        let r = row.lead(m::AVATAR, m::AVATAR, gap);
        // The last one is you.
        let pic = if i + 1 == n {
            avatar::mine(snap)
        } else {
            avatar::of_name(snap, name)
        };
        speaker(ui, r, name, *paper, pic, *speaking, alpha);
    }
    // The buttons, from the end: leave, share, stop hearing, mute.
    let ringing = call.since.is_none();
    let share_on = snap.live.share.is_some();
    let end = paint::call_button(
        ui,
        row.trail(m::CC, m::CC, m::CC_GAP),
        Id::unique("chat.tray.end"),
        icons::PHONE_END,
        Cc::End,
        false,
        s::LEAVE_CALL(),
        if ringing {
            s::CANCEL_CALL()
        } else {
            s::LEAVE_CALL()
        },
    );
    // Sharing in a call is not carried (the stage's button says why): only Stop sharing.
    let share = (snap.features.share && share_on).then(|| {
        paint::call_button(
            ui,
            row.trail(m::CC, m::CC, m::CC_GAP),
            Id::unique("chat.tray.share"),
            icons::SHARE_GAME,
            Cc::Share,
            true,
            s::SHARE_TIP(),
            s::STOP_SHARE_TIP(),
        )
    });
    let deaf = paint::call_button(
        ui,
        row.trail(m::CC, m::CC, m::CC_GAP),
        Id::unique("chat.tray.deaf"),
        icons::HEADPHONES,
        Cc::Slash,
        call.deafened,
        s::DEAFEN(),
        if call.deafened {
            s::UNDEAFEN()
        } else {
            s::DEAFEN()
        },
    );
    let mute = paint::call_button(
        ui,
        row.trail(m::CC, m::CC, m::CC_GAP),
        Id::unique("chat.tray.mute"),
        icons::MIC,
        Cc::Slash,
        call.muted,
        s::MUTE_MIC(),
        if call.muted { s::UNMUTE() } else { s::MUTE() },
    );
    let larger = paint::call_button(
        ui,
        row.trail(m::CC, m::CC, m::GAP),
        Id::unique("chat.tray.larger"),
        icons::EXPAND,
        Cc::Plain,
        false,
        cs::LARGER(),
        cs::LARGER(),
    );
    // Who and how long.
    let firsts: Vec<&str> = call.with.iter().map(|n| paint::first_name(n)).collect();
    let names = s::and_list(&firsts);
    let (title, sub) = if ringing {
        (s::calling(&names), s::RINGING().to_owned())
    } else {
        let ms = call
            .since
            .and_then(|since| env.now.since(since))
            .map_or(0, |d| d.as_millis() as u64);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1_000 - ms % 1_000));
        let title = if firsts.is_empty() {
            s::JUST_YOU().to_owned()
        } else {
            s::with_you(&names)
        };
        let game = snap.live.share.as_ref().map(|s| s.title.as_str());
        (title, s::call_line(&paint::clock(ms), game, call.muted))
    };
    two_lines(ui, row.middle(), &title, &sub, &p, alpha, row.rtl);
    if mute.clicked() {
        env.bridge
            .send(Command::Chat(ChatCommand::CallMute(!call.muted)));
    }
    if deaf.clicked() {
        env.bridge
            .send(Command::Chat(ChatCommand::CallDeafen(!call.deafened)));
    }
    if share.is_some_and(|r| r.clicked()) {
        super::dialogs::share_flow(state, env, snap, call.chat);
    }
    if end.clicked() {
        env.bridge.send(Command::Chat(ChatCommand::EndCall));
    }
    if larger.clicked() {
        state.chat.call.smaller = false;
    }
}

/// A group's voice you have not joined: who is talking, and Join.
fn voice_tray(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    inset: f32,
) {
    let ChatRef::Group(g) = chat else { return };
    let Some(group) = snap.group(g) else { return };
    let p = theme::theme(ui.ctx()).palette.clone();
    let (rect, alpha) = plate(ui, Id::unique(("chat.tray.voice", chat)), inset);
    let mut row = Row::new(rect);
    let talking: Vec<(String, Option<u8>)> = group
        .voice
        .iter()
        .filter_map(|id| group.member_list.iter().find(|m| m.id == *id))
        .map(|m| {
            let paper = m.friend.and_then(|f| snap.friend(f)).and_then(|f| f.paper);
            (m.name.clone(), paper)
        })
        .collect();
    let n = talking.len();
    for (i, (name, paper)) in talking.iter().enumerate() {
        let gap = if i + 1 == n { m::GAP } else { m::AVATAR_GAP };
        let r = row.lead(m::AVATAR, m::AVATAR, gap);
        let pic = avatar::of_name(snap, name);
        avatar::pictured(ui.painter(), r, name, *paper, AvShape::Person, pic);
    }
    let join = Button::primary(s::JOIN());
    let bw = join_width(ui);
    let br = row.trail(bw, theme::tokens::control::BUTTON, m::GAP);
    let clicked = paint::place(ui, br, join).clicked();
    let firsts: Vec<&str> = talking.iter().map(|(n, _)| paint::first_name(n)).collect();
    two_lines(
        ui,
        row.middle(),
        &s::talking(&s::and_list(&firsts), n),
        s::VOICE_HERE(),
        &p,
        alpha,
        row.rtl,
    );
    if clicked {
        super::dialogs::call_flow(state, env, snap, chat);
    }
}

/// The width of the Join button.
fn join_width(ui: &Ui) -> f32 {
    let g = Type::BodyBold.line(ui, s::JOIN(), Color32::PLACEHOLDER);
    g.size().x + theme::tokens::control::BUTTON_PAD * 2.0
}

/// Your own sharing outside a call: the game (dimmed with a pause mark while it is not in
/// front), who watches, and Stop sharing.
fn share_tray(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    sh: &easyplay_core::model::Share,
    inset: f32,
) {
    let th = theme::theme(ui.ctx());
    let p = th.palette.clone();
    let (rect, alpha) = plate(ui, Id::unique(("chat.tray.share", sh.chat)), inset);
    let mut row = Row::new(rect);
    let paused = ui.ctx().animate_bool_with_time(
        Id::unique(("chat.tray.share.paused", sh.chat)),
        snap.live.stream.share_paused,
        th.motion.fade(Dur::Base),
    );
    let cover = row.lead(
        m::COVER,
        m::COVER * theme::tokens::layout::COVER_ASPECT,
        m::GAP,
    );
    crate::widgets::cover::paint(
        ui.painter(),
        cover,
        crate::widgets::cover::CoverSource::Placeholder,
        &sh.title,
        crate::widgets::cover::CoverLook {
            opacity: alpha,
            ..Default::default()
        },
        &th,
    );
    if paused > 0.0 {
        // No live picture of your own share is kept: the cover stands for it.
        let [_, light] = m::PAUSED;
        let corner = CornerRadius::same(theme::tokens::radius::SM);
        ui.painter().rect_filled(
            cover,
            corner,
            fade(Color32::BLACK, (1.0 - light) * paused * alpha),
        );
        ui.painter().text(
            cover.center(),
            Align2::CENTER_CENTER,
            icons::PAUSE.text(true),
            icons::font(11.0),
            fade(Color32::WHITE, paused * alpha),
        );
    }
    let label = s::STOP_SHARING();
    let bw = Type::BodyStrong
        .line(ui, label, Color32::PLACEHOLDER)
        .size()
        .x
        + theme::tokens::control::BUTTON_PAD * 2.0;
    let br = row.trail(bw, theme::tokens::control::BUTTON, m::GAP);
    let stop = paint::place(ui, br, Button::new(label)).clicked();
    // What is sent, as a select of the size and the rate (notes J).
    let prefs = env.bridge.prefs().clone();
    let (text, lower, tip) = super::share_quality::chip_text(&snap.live.stream, &prefs);
    let qw = super::share_quality::chip_width(ui, &text);
    let qr = row.trail(qw, 28.0, m::GAP);
    super::share_quality::chip(ui, env, qr, &text, lower, tip.as_deref());
    // The eye and the count, before the chip.
    let count = Type::Body.numeric_galley(ui, &sh.watching.to_string(), p.t3);
    let ew = 16.0 + 4.0 + count.size().x;
    let er = row.trail(ew, m::AVATAR, m::GAP);
    let eye = ui.interact(er, Id::unique("chat.tray.share.eye"), Sense::hover());
    crate::widgets::tooltip::attach(eye, &s::watching(sh.watching));
    let (ix, cx) = if row.rtl {
        (er.right() - 8.0, er.left())
    } else {
        (er.left() + 8.0, er.right() - count.size().x)
    };
    ui.painter().text(
        pos2(ix, er.center().y),
        Align2::CENTER_CENTER,
        icons::EYE.text(false),
        icons::font(13.0),
        fade(p.t3, alpha),
    );
    ui.painter().galley(
        pos2(cx, er.center().y - count.size().y / 2.0),
        count,
        fade(p.t3, alpha),
    );
    let sub = if snap.live.stream.share_paused {
        cs::PAUSED().to_owned()
    } else if sh.watching == 0 {
        format!("{} {}", s::SHARE_ONLY(), s::NOBODY_WATCHING())
    } else {
        format!("{} {}", s::SHARE_ONLY(), cs::watching(sh.watching))
    };
    two_lines(
        ui,
        row.middle(),
        &s::you_share(&sh.title),
        &sub,
        &p,
        alpha,
        row.rtl,
    );
    if stop {
        env.bridge.send(Command::Chat(ChatCommand::StopShare));
        state.toast(s::STOPPED_SHARING(), ToastKind::Info, None, env.mono);
    }
}
