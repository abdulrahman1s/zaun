//! The trays under a conversation's header (chat.html section 8 "trays", script section
//! 25 `traysHTML`): the call you are in, a group's voice you have not joined, your own
//! sharing, and the stage of the game you watch. Owner: the details agent.
//!
//! `show` is given the conversation's whole column and sets its own inset (24, 12 in a
//! narrow window) and the 10 under each tray; the stage centres in the column.

use easyplay_core::model::{ChatRef, ChatSnapshot, Watching};
use easyplay_core::service::ChatCommand;
use egui::{
    Align2, Color32, CornerRadius, Id, Key, Modifiers, Painter, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use super::Env;
use super::avatar::{self, Shape as AvShape};
use super::paint::{self, Cc};
use crate::app::UiState;
use crate::bridge::Command;
use crate::strings::chat::details as s;
use crate::theme::palette::{self, Palette, fade, mix};
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
    /// The stage takes at most this share of the window's height…
    pub const STAGE_H: f32 = 0.46;
    /// …or this one, larger.
    pub const STAGE_H_LARGE: f32 = 0.72;
    /// The stage's radius.
    pub const STAGE_RADIUS: u8 = 12;
    /// The stage's chips: 10 in, 28 high, 9 padding, radius 8, 6 apart.
    pub const CHIP: [f32; 4] = [10.0, 28.0, 9.0, 6.0];
    /// The chips' plate: black at 60 %, 80 % under the pointer.
    pub const CHIP_ALPHA: [f32; 2] = [0.6, 0.8];
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
    if f.calls {
        if let Some(call) = call {
            call_tray(ui, state, env, snap, call, inset);
        } else if let ChatRef::Group(g) = chat
            && let Some(group) = snap.group(g)
            && !group.voice.is_empty()
        {
            voice_tray(ui, state, env, snap, chat, inset);
        }
    }
    if f.share {
        if let Some(sh) = snap.live.share.as_ref().filter(|s| s.chat == chat)
            && call.is_none()
        {
            share_tray(ui, state, env, sh, inset);
        }
        if let Some(w) = snap.live.watch.as_ref().filter(|w| w.chat == chat) {
            stage(ui, state, env, w, inset);
        }
    }
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
fn speaker(ui: &Ui, r: Rect, name: &str, paper: Option<u8>, speaking: bool, alpha: f32) {
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
    avatar::avatar(ui.painter(), r, name, paper, AvShape::Person);
}

/// The call you are in: who, the clock, and the round buttons.
fn call_tray(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    call: &easyplay_core::model::Call,
    inset: f32,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (rect, alpha) = plate(ui, Id::unique(("chat.tray.call", call.chat)), inset);
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
        speaker(ui, r, name, *paper, *speaking, alpha);
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
    let share = snap.features.share.then(|| {
        paint::call_button(
            ui,
            row.trail(m::CC, m::CC, m::CC_GAP),
            Id::unique("chat.tray.share"),
            icons::SHARE_GAME,
            Cc::Share,
            share_on,
            s::SHARE_TIP(),
            if share_on {
                s::STOP_SHARE_TIP()
            } else {
                s::SHARE_TIP()
            },
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
        row.trail(m::CC, m::CC, m::GAP),
        Id::unique("chat.tray.mute"),
        icons::MIC,
        Cc::Slash,
        call.muted,
        s::MUTE_MIC(),
        if call.muted { s::UNMUTE() } else { s::MUTE() },
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
        avatar::avatar(ui.painter(), r, name, *paper, AvShape::Person);
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

/// Your own sharing outside a call: the game, who watches, and Stop sharing.
fn share_tray(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    sh: &easyplay_core::model::Share,
    inset: f32,
) {
    let th = theme::theme(ui.ctx());
    let p = th.palette.clone();
    let (rect, alpha) = plate(ui, Id::unique(("chat.tray.share", sh.chat)), inset);
    let mut row = Row::new(rect);
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
    let label = s::STOP_SHARING();
    let bw = Type::BodyStrong
        .line(ui, label, Color32::PLACEHOLDER)
        .size()
        .x
        + theme::tokens::control::BUTTON_PAD * 2.0;
    let br = row.trail(bw, theme::tokens::control::BUTTON, m::GAP);
    let stop = paint::place(ui, br, Button::new(label)).clicked();
    let who = if sh.watching == 0 {
        s::NOBODY_WATCHING().to_owned()
    } else {
        s::watching(sh.watching)
    };
    two_lines(
        ui,
        row.middle(),
        &s::you_share(&sh.title),
        &format!("{} {who}", s::SHARE_ONLY()),
        &p,
        alpha,
        row.rtl,
    );
    if stop {
        env.bridge.send(Command::Chat(ChatCommand::StopShare));
        state.toast(s::STOPPED_SHARING(), ToastKind::Info, None, env.mono);
    }
}

/// The game you watch: a 16:9 stage, centred, at most 46 % of the window's height (72 %
/// larger), with its chips; sound, size and Stop watching along the bottom while pointed
/// at.
fn stage(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, w: &Watching, inset: f32) {
    let th = theme::theme(ui.ctx());
    let large = state.chat.stage_large;
    // Esc shrinks the stage once nothing nearer the field wants it.
    if large
        && state.chat.picker.is_none()
        && state.chat.recording.is_none()
        && state.chat.reply.is_none()
        && state.chat.edit.is_none()
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        state.chat.stage_large = false;
    }
    let col = ui.available_width();
    let share = if large { m::STAGE_H_LARGE } else { m::STAGE_H };
    let max_w = ui.ctx().content_rect().height() * share * 16.0 / 9.0;
    let sw = (col - inset * 2.0).min(max_w).max(1.0);
    let sh = sw * 9.0 / 16.0;
    let alpha = motion::appear(
        ui.ctx(),
        Id::unique(("chat.stage", w.chat)),
        th.motion.fade(Dur::Base),
    );
    let (row, _) = ui.allocate_exact_size(vec2(col, sh + m::BELOW), Sense::hover());
    let rect = Rect::from_min_size(pos2(row.center().x - sw / 2.0, row.top()), vec2(sw, sh));
    let resp = ui.interact(rect, Id::unique("chat.stage"), Sense::hover());
    let who = s::stage_who(paint::first_name(&w.name), &w.title);
    crate::a11y::item(&resp, egui::Role::Image, &who);
    let painter = ui.painter().clone();
    still(&painter, rect, &w.title, alpha);
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let [inn, ch, _, gap] = m::CHIP;
    // Top: "● Live", "Jonas, Ninefold Arena"; the eye and the count at the other end.
    let mut x = 0.0;
    let place = |from: f32, cw: f32, top: bool| -> Rect {
        let left = if rtl {
            rect.right() - inn - from - cw
        } else {
            rect.left() + inn + from
        };
        let y = if top {
            rect.top() + inn
        } else {
            rect.bottom() - inn - ch
        };
        Rect::from_min_size(pos2(left, y), vec2(cw, ch))
    };
    let place_end = |from_end: f32, cw: f32, top: bool| -> Rect {
        place(sw - inn * 2.0 - from_end - cw, cw, top)
    };
    let live_w = {
        let g = live_galley(&painter, s::LIVE());
        6.0 + 5.0 + g.size().x
    };
    let r = place(x, live_w + m::CHIP[2] * 2.0, true);
    chip_plate(&painter, r, 0.0, alpha);
    paint::live_mark(
        &painter,
        r.left() + m::CHIP[2],
        r.center().y,
        s::LIVE(),
        fade(paint::LIVE_ON_DARK, alpha),
    );
    x += r.width() + gap;
    let g = Type::BodyStrong.galley(ui, &who, Color32::WHITE, (sw * 0.6).max(40.0), 1);
    let r = place(x, g.size().x + m::CHIP[2] * 2.0, true);
    chip_plate(&painter, r, 0.0, alpha);
    painter.galley_with_override_text_color(
        pos2(r.left() + m::CHIP[2], r.center().y - g.size().y / 2.0),
        g,
        fade(Color32::WHITE, alpha),
    );
    let count = Type::BodyStrong.numeric_galley(ui, &w.watching.to_string(), Color32::WHITE);
    let ew = 14.0 + 6.0 + count.size().x + m::CHIP[2] * 2.0;
    let r = place_end(0.0, ew, true);
    chip_plate(&painter, r, 0.0, alpha);
    let eye = ui.interact(r, Id::unique("chat.stage.count"), Sense::hover());
    crate::widgets::tooltip::attach(eye, &s::watching(w.watching));
    painter.text(
        pos2(r.left() + m::CHIP[2] + 7.0, r.center().y),
        Align2::CENTER_CENTER,
        icons::EYE.text(false),
        icons::font(14.0),
        fade(Color32::WHITE, alpha),
    );
    painter.galley_with_override_text_color(
        pos2(
            r.left() + m::CHIP[2] + 14.0 + 6.0,
            r.center().y - count.size().y / 2.0,
        ),
        count,
        fade(Color32::WHITE, alpha),
    );
    // Bottom, while pointed at or holding focus: sound · larger · Stop watching.
    let focus_in = ui.ctx().memory(|mem| {
        mem.focused().is_some_and(|f| {
            [
                Id::unique("chat.stage.sound"),
                Id::unique("chat.stage.size"),
                Id::unique("chat.stage.stop"),
            ]
            .contains(&f)
        })
    });
    let pointed = ui.rect_contains_pointer(rect);
    let shown = ui.ctx().animate_bool_with_time(
        Id::unique("chat.stage.bottom"),
        pointed || focus_in,
        th.motion.fade(Dur::Fast),
    );
    let b_alpha = alpha * shown;
    let sound_r = place(0.0, ch, false);
    let sound = stage_button(
        ui,
        sound_r,
        "chat.stage.sound",
        StageGlyph::Icon(if w.muted {
            icons::SPEAKER_OFF
        } else {
            icons::SPEAKER
        }),
        if w.muted {
            s::SOUND_ON()
        } else {
            s::SOUND_OFF()
        },
        b_alpha,
    );
    let stop_g = Type::BodyStrong.line(ui, s::STOP_WATCHING(), Color32::WHITE);
    let stop_r = place_end(0.0, stop_g.size().x + m::CHIP[2] * 2.0, false);
    let stop = stage_button(
        ui,
        stop_r,
        "chat.stage.stop",
        StageGlyph::Text(stop_g),
        s::STOP_WATCHING(),
        b_alpha,
    );
    let size_r = place_end(stop_r.width() + gap, ch, false);
    let size = stage_button(
        ui,
        size_r,
        "chat.stage.size",
        StageGlyph::Icon(if large { icons::SHRINK } else { icons::EXPAND }),
        if large { s::SMALLER() } else { s::LARGER() },
        b_alpha,
    );
    if sound.clicked() {
        env.bridge
            .send(Command::Chat(ChatCommand::WatchMute(!w.muted)));
    }
    if size.clicked() {
        state.chat.stage_large = !large;
    }
    if stop.clicked() {
        state.chat.stage_large = false;
        env.bridge.send(Command::Chat(ChatCommand::StopWatch));
    }
}

fn live_galley(painter: &Painter, label: &str) -> std::sync::Arc<egui::Galley> {
    painter.layout_no_wrap(
        label.to_owned(),
        egui::FontId::new(11.0, egui::FontFamily::Name("w600".into())),
        Color32::WHITE,
    )
}

/// A chip's black plate; `hover` 0–1 darkens it.
fn chip_plate(painter: &Painter, r: Rect, hover: f32, alpha: f32) {
    let [rest, over] = m::CHIP_ALPHA;
    let a = rest + (over - rest) * hover;
    painter.rect_filled(
        r,
        CornerRadius::same(theme::tokens::radius::MD),
        fade(Color32::BLACK, a * alpha),
    );
}

enum StageGlyph {
    Icon(theme::Icon),
    Text(std::sync::Arc<egui::Galley>),
}

/// A chip that acts (`button.sb`), at `alpha`.
fn stage_button(
    ui: &mut Ui,
    r: Rect,
    id: &str,
    glyph: StageGlyph,
    name: &str,
    alpha: f32,
) -> egui::Response {
    let th = theme::theme(ui.ctx());
    // Hidden at rest, but still there for the keyboard (focus reveals the row).
    let resp = ui.interact(r, Id::unique(id), Sense::click());
    crate::a11y::button(&resp, name);
    let resp = if matches!(glyph, StageGlyph::Icon(_)) {
        crate::widgets::tooltip::attach(resp, name)
    } else {
        resp
    };
    let hover = crate::widgets::hover_t(&resp, &th);
    let painter = ui.painter();
    chip_plate(painter, r, hover, alpha);
    match glyph {
        StageGlyph::Icon(i) => {
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                i.text(false),
                icons::font(14.0),
                fade(Color32::WHITE, alpha),
            );
        }
        StageGlyph::Text(g) => {
            painter.galley_with_override_text_color(
                pos2(r.left() + m::CHIP[2], r.center().y - g.size().y / 2.0),
                g,
                fade(Color32::WHITE, alpha),
            );
        }
    }
    if resp.has_focus() {
        crate::widgets::focus_ring(
            painter,
            r,
            CornerRadius::same(theme::tokens::radius::MD),
            &th,
        );
    }
    resp
}

/// A still stand-in for the live picture: the game's paper with two hills and a sun in its
/// ink (the prototype's picture, not moving).
fn still(painter: &Painter, rect: Rect, title: &str, alpha: f32) {
    let palette::Paper { paper, ink } = palette::paper_for(title);
    let corner = CornerRadius::same(m::STAGE_RADIUS);
    painter.rect_filled(rect, corner, fade(Color32::BLACK, alpha));
    painter.rect_filled(rect, corner, fade(paper, alpha));
    let (w, h) = (rect.width(), rect.height());
    // The sun: 13 % of the width, 62 % across and 16 % down.
    let d = w * 0.13;
    painter.circle_filled(
        pos2(
            rect.left() + w * 0.62 + d / 2.0,
            rect.top() + h * 0.16 + d / 2.0,
        ),
        d / 2.0,
        fade(mix(paper, ink, 0.8), alpha),
    );
    hill(painter, rect, 0.40, fade(mix(paper, ink, 0.22), alpha));
    hill(painter, rect, 0.26, fade(mix(paper, ink, 0.5), alpha));
}

/// A hill: the top half of an ellipse twice the stage's width and `share` of its height,
/// standing on its bottom, inside its rounded corners.
fn hill(painter: &Painter, rect: Rect, share: f32, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let (cx, base, rx, ry) = (rect.center().x, rect.bottom(), w, h * share);
    let r = f32::from(m::STAGE_RADIUS);
    let mut pts: Vec<Pos2> = Vec::new();
    let steps = 32;
    for i in 0..=steps {
        let x = rect.left() + w * i as f32 / steps as f32;
        let dx = (x - cx) / rx;
        let y = base - ry * (1.0 - dx * dx).max(0.0).sqrt();
        pts.push(pos2(x, y.max(rect.top())));
    }
    // Down the right side and round the bottom corners.
    for i in 0..=8 {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
        pts.push(pos2(rect.right() - r + r * a.cos(), base - r + r * a.sin()));
    }
    for i in 0..=8 {
        let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
        pts.push(pos2(rect.left() + r + r * a.cos(), base - r + r * a.sin()));
    }
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}
