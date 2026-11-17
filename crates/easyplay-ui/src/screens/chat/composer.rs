//! The composer (chat.html section 8 "composer", CSS `.cmp*`, `.cb`, `.fmt`, `.rec-row`,
//! `.cmp-off`, `.cmp-net`, `.ac`; script sections 23–24 `composerHTML`, `typed`, `drawAc`,
//! `acPick`, `format`, `send`, `rec*`, and section 27's keys).
//!
//! One field, radius 14, on the well with a 1 pt line (1.5 accent mark while focused): + ·
//! the text (14/20, grows to eight lines) · Aa · GIF · emoji · the microphone, which turns
//! into the filled Send arrow as soon as there is text. Round 32 buttons. On top, a 34 pt
//! bar while replying or editing; under it, the formatting row while Aa is on. While
//! recording, the row is a coral dot, the time, the waveform growing from the right, "5 min
//! at most", discard and send. A blocked conversation shows why instead, with Unblock;
//! offline, an amber line sits above the field. `:na` suggests emoji names and, in groups,
//! `@na` suggests members, above the field.
//!
//! Keys: Enter sends (Ctrl+Enter when Settings › Chat › Send with says so), Shift+Enter
//! breaks the line, Ctrl+B / I / E format, ↑ in an empty field edits the last text message
//! the user sent; in the suggestions ↑ ↓ choose, Enter or Tab inserts, Esc dismisses. Esc
//! is the page's (`ChatVm::escape`, run by the shell before any widget): the suggestions,
//! the picker, the recording, the reply or edit, in that order; the composer then gives
//! the field its focus back and keeps dismissed suggestions away until the word changes.
//! Typing while nothing has focus goes to the field. A focus request (opening a chat)
//! stands for two passes. Every control is drawn only when the conversation's features
//! carry it.
//!
//! The emoji picker is drawn at the end of [`show`] (see `emoji.rs`).

use easyplay_core::config::schema::ChatSendWith;
use easyplay_core::model::{
    ChatFeatures, ChatRef, ChatSnapshot, Friend, GroupMember, MessageAuthor, MessageBody,
};
use easyplay_core::service::{ChatCommand, ClipKind};
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{
    Align, Align2, Color32, Context, CornerRadius, Event, EventFilter, FontFamily, FontId, Id, Key,
    Modifiers, Order, Rect, Response, Sense, Stroke, StrokeKind, TextBuffer, Ui, UiBuilder, Vec2,
    pos2, vec2,
};

use super::Env;
use super::emoji;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::i18n::{self, Dir};
use crate::input::picker::PickPurpose;
use crate::strings::chat::composer as s;
use crate::strings::chat::list as ls;
use crate::theme::palette::Palette;
use crate::theme::tokens::{icon as icon_size, stroke};
use crate::theme::{self, Icon, Type, icons, motion};
use crate::vm::chat::{Attachment, Clip, ClipboardSource, Picker, Suggest, SuggestKind};
use crate::widgets::button::Button;
use crate::widgets::{context_menu, tooltip};

/// What the composer may do in this conversation.
pub enum Compose {
    /// Write.
    Open,
    /// Not now, for this reason.
    Closed(String),
}

/// The composer's measures (CSS section 13 "The composer").
mod m {
    /// `.cmp`: radius.
    pub const RADIUS: u8 = 14;
    /// `.cmp-bar`: height, padding before and after, gap, glyph, the × button.
    pub const BAR_H: f32 = 34.0;
    pub const BAR_PAD: [f32; 2] = [14.0, 4.0];
    pub const BAR_GAP: f32 = 8.0;
    pub const BAR_ICON: f32 = 12.0;
    pub const BAR_CLOSE: f32 = 26.0;
    /// The attachment strip: padding (above and at the sides), space under the tiles (the
    /// text line's own padding follows), tiles 80 square, 8 apart, radius 10, popping in
    /// from 90 %; a file's chip 200 × 56 with a 36 icon tile (radius 8) and 10 padding,
    /// leaving room for the corner ×: a 20 disc hanging 6 over, its glyph 12. A video's play
    /// disc.
    pub const STRIP_PAD: f32 = 12.0;
    /// Formatting as it is typed: `code` in mono 13, `!- small!` at 12.
    pub const LIVE_CODE: f32 = 13.0;
    pub const LIVE_SMALL: f32 = 12.0;
    pub const STRIP_BELOW: f32 = 4.0;
    pub const TILE: f32 = 80.0;
    pub const TILE_GAP: f32 = 8.0;
    pub const TILE_RADIUS: u8 = 10;
    pub const TILE_FROM: f32 = 0.9;
    pub const CHIP: [f32; 2] = [200.0, 56.0];
    pub const CHIP_ICON: f32 = 36.0;
    pub const CHIP_ICON_RADIUS: u8 = 8;
    pub const CHIP_PAD: f32 = 10.0;
    pub const CORNER: f32 = 20.0;
    pub const CORNER_HANG: f32 = 6.0;
    pub const CORNER_GLYPH: f32 = 12.0;
    pub const CORNER_ROOM: f32 = 8.0;
    pub const PLAY_DISC: f32 = 32.0;
    /// The drop overlay: the wall's opacity, the dashed outline's inset, dash and gap, the
    /// glyph, and the gap under it.
    pub const DROP_WALL: f32 = 0.85;
    pub const DROP_INSET: f32 = 16.0;
    pub const DROP_DASH: [f32; 2] = [8.0, 6.0];
    pub const DROP_ICON: f32 = 32.0;
    pub const DROP_GAP: f32 = 12.0;
    /// `.fmt`: padding top and sides, buttons, their radius, glyphs, gap.
    pub const FMT_PAD: [f32; 2] = [5.0, 8.0];
    pub const FMT_BTN: f32 = 26.0;
    pub const FMT_RADIUS: u8 = 6;
    pub const FMT_ICON: f32 = 14.0;
    pub const FMT_GAP: f32 = 2.0;
    pub const FMT_HINT_PAD: f32 = 6.0;
    /// The colour button's ring of six dots, and a colour's swatch in its menu.
    pub const COLOR_RING: f32 = 5.0;
    pub const COLOR_DOT: f32 = 1.8;
    pub const COLOR_SWATCH: f32 = 5.0;
    /// `.cmp-row`: padding (vertical, horizontal), gap; `.cb`.
    pub const ROW_PAD: [f32; 2] = [5.0, 6.0];
    pub const ROW_GAP: f32 = 2.0;
    pub const BTN: f32 = 32.0;
    /// The text: padding, at most eight lines of 20 (and the padding).
    pub const TEXT_PAD: f32 = 6.0;
    pub const TEXT_MAX: f32 = 172.0;
    /// `.cb-g`: the GIF word's box.
    pub const GIF_PAD: [f32; 2] = [4.0, 2.0];
    pub const GIF_SIZE: f32 = 9.0;
    pub const GIF_RADIUS: u8 = 4;
    /// `.rec-row`.
    pub const REC_H: f32 = 42.0;
    pub const REC_PAD: [f32; 2] = [16.0, 6.0];
    pub const REC_GAP: f32 = 10.0;
    pub const REC_DOT: f32 = 8.0;
    pub const REC_TIME_W: f32 = 34.0;
    pub const REC_MAX_S: f64 = 300.0;
    /// A new bar every 90 ms; at most 160 kept.
    pub const REC_TICK: f64 = 0.09;
    pub const REC_BARS: usize = 160;
    /// `.wave i`: bars 2 wide, 2 apart, in 24.
    pub const BAR_W: f32 = 2.0;
    pub const BAR_STEP: f32 = 4.0;
    pub const WAVE_H: f32 = 24.0;
    /// The dot's pulse: alpha 1 ↔ .45 over 1.2 s.
    pub const PULSE_S: f64 = 1.2;
    pub const PULSE_LOW: f32 = 0.45;
    /// `.cmp-off`.
    pub const OFF_MIN_H: f32 = 44.0;
    pub const OFF_PAD: [f32; 2] = [14.0, 6.0];
    pub const OFF_GAP: f32 = 12.0;
    /// `.cmp-net`.
    pub const NET_ICON: f32 = 13.0;
    pub const NET_GAP: f32 = 8.0;
    pub const NET_BELOW: f32 = 8.0;
    /// `.ac`: inset from the field, gap above it, least width, padding, radius; `.mi`.
    pub const AC_X: f32 = 6.0;
    pub const AC_GAP: f32 = 6.0;
    pub const AC_MIN_W: f32 = 220.0;
    pub const AC_PAD: f32 = 5.0;
    pub const AC_RADIUS: u8 = 10;
    pub const ITEM_H: f32 = 28.0;
    pub const ITEM_PAD: [f32; 2] = [8.0, 10.0];
    pub const ITEM_GAP: f32 = 8.0;
    pub const ITEM_RADIUS: u8 = 6;
    pub const AC_EMOJI: f32 = 18.0;
    pub const SUGGEST_MAX: usize = 6;
}

/// The message field's id (one for every conversation; each keeps its own draft).
pub fn field_id() -> Id {
    Id::unique("chat.composer.field")
}

/// The composer: the field and its buttons, the recording, or why there is none. Draws the
/// emoji picker last.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    chat: ChatRef,
    name: &str,
    compose: &Compose,
) {
    let ctx = ui.ctx().clone();
    let snap = env.bridge.chat().clone();
    let prefs = env.bridge.prefs().clone();
    emoji::set_system(&ctx, emoji::system_style(&prefs));
    let f = snap.features;
    let p = theme::theme(&ctx).palette.clone();
    let rtl = i18n::dir() == Dir::Rtl;
    // Messages with pictures, built off the UI thread, go out now (in order).
    for cmd in encoded(&ctx).take() {
        env.bridge.send(UiCommand::Chat(cmd));
    }
    if !f.voice {
        state.chat.recording = None;
    }
    // Another chat: what belonged to the last one goes (an edit gives its text back).
    let last: Option<ChatRef> = ctx.data(|d| d.get_temp(Id::unique("chat.composer.chat")));
    ctx.data_mut(|d| d.insert_temp(Id::unique("chat.composer.chat"), chat));
    if let Some(last) = last
        && last != chat
    {
        if state.chat.edit.take().is_some() {
            state.chat.drafts.remove(&last);
        }
        state.chat.reply = None;
        state.chat.suggest = None;
        state.chat.recording = None;
        state.chat.picker = None;
        state.chat.attachments.clear();
    }
    // Attachments the page's Esc removed: focus comes back to the field.
    let held = ctx.data(|d| d.get_temp::<bool>(Id::unique("chat.composer.held")));
    let held_now = !state.chat.attachments.is_empty();
    ctx.data_mut(|d| d.insert_temp(Id::unique("chat.composer.held"), held_now));
    if held == Some(true) && !held_now {
        state.chat.focus_composer = true;
    }
    // A recording the page's Esc discarded: focus comes back to the field.
    let recorded = ctx.data(|d| d.get_temp::<bool>(Id::unique("chat.composer.recorded")));
    let recording_now = state.chat.recording.is_some();
    ctx.data_mut(|d| d.insert_temp(Id::unique("chat.composer.recorded"), recording_now));
    if recorded == Some(true) && !recording_now {
        state.chat.focus_composer = true;
    }
    // A picker the composer opened, closed by the page's Esc: focus comes back to the field.
    let inserting = matches!(
        state.chat.picker,
        Some(Picker::Emoji { react_to: None } | Picker::Gif)
    );
    let was = ctx.data(|d| d.get_temp::<bool>(Id::unique("chat.composer.picker")));
    ctx.data_mut(|d| d.insert_temp(Id::unique("chat.composer.picker"), inserting));
    if was == Some(true) && state.chat.picker.is_none() {
        state.chat.focus_composer = true;
    }

    // Esc after the picker (which takes its own): the recording, then the reply or edit.
    if state.chat.picker.is_none() && state.chat.suggest.is_none() {
        if state.chat.recording.is_some() {
            if consume(&ctx, Key::Escape) {
                state.chat.recording = None;
                state.chat.focus_composer = true;
            }
        } else if (state.chat.reply.is_some() || state.chat.edit.is_some())
            && consume(&ctx, Key::Escape)
        {
            cancel_bar(state, chat);
        }
    }

    let blocked = match chat {
        ChatRef::Direct(id) => snap.friend(id).filter(|x| x.blocked),
        ChatRef::Group(_) => None,
    };
    // Files dropped on the window go to this conversation (see `take_drop`).
    // The conversation's column: its width is the composer's clip; its height runs from under
    // the top bar to the window's bottom.
    let column = {
        let clip = ui.clip_rect();
        let screen = ctx.content_rect();
        Rect::from_x_y_ranges(
            clip.x_range(),
            (screen.top() + theme::tokens::layout::TOP_BAR)..=screen.bottom(),
        )
    };
    let drops = f.files && blocked.is_none() && matches!(compose, Compose::Open);
    if drops {
        take_drop(&ctx, state, env);
    }
    if let Some(friend) = blocked {
        blocked_bar(ui, env, &p, friend, f.block, rtl);
    } else {
        if snap.offline {
            offline_line(ui, &p, rtl);
        }
        match compose {
            Compose::Closed(why) => closed_bar(ui, &p, why, rtl),
            Compose::Open if state.chat.recording.is_some() => {
                recording(ui, state, env, chat, &p, rtl);
            }
            Compose::Open => field(ui, state, env, &snap, chat, name, &p, rtl),
        }
    }
    emoji::picker(&ctx, state, env);
    super::gif::picker(&ctx, state, env);
    if drops {
        let to = if matches!(chat, ChatRef::Group(_)) {
            name
        } else {
            first(name)
        };
        drop_overlay(&ctx, column, to);
    }
}

/// Take a plain press of `key` from this pass's input.
fn consume(ctx: &Context, key: Key) -> bool {
    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key))
}

/// × or Esc on the reply or edit bar: an edit gives its text back.
fn cancel_bar(state: &mut UiState, chat: ChatRef) {
    if state.chat.edit.take().is_some() {
        state.chat.drafts.entry(chat).or_default().clear();
    }
    state.chat.reply = None;
    state.chat.focus_composer = true;
}

/// Start editing one of the user's text messages: its text goes into the field (the
/// conversation's Edit and ↑ in an empty field).
pub fn start_edit(state: &mut UiState, chat: ChatRef, item: i64, text: &str) {
    state.chat.edit = Some(item);
    state.chat.reply = None;
    state.chat.suggest = None;
    state.chat.drafts.insert(chat, text.to_owned());
    state.chat.focus_composer = true;
}

/// Put `text` at the field's cursor in `chat`'s draft (the emoji picker).
pub(super) fn insert(ctx: &Context, state: &mut UiState, chat: ChatRef, text: &str) {
    let draft = state.chat.drafts.entry(chat).or_default();
    emoji::insert_at_cursor(ctx, field_id(), draft, text);
}

/// A friend's first name (the placeholder, the blocked line).
fn first(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// `x` from the start edge of `outer` (the left one, the right one in Arabic), for something
/// `w` wide.
fn at(outer: Rect, x: f32, w: f32, rtl: bool) -> f32 {
    if rtl {
        outer.right() - x - w
    } else {
        outer.left() + x
    }
}

// ---- Instead of the field -------------------------------------------------------------

/// `.cmp-off`: an o1 plate with a glyph, a sentence and maybe one button.
fn plate(
    ui: &mut Ui,
    p: &Palette,
    icon: Icon,
    text: &str,
    button: Option<&str>,
    rtl: bool,
) -> bool {
    let w = ui.available_width();
    let glyph = icon_size::INLINE;
    let button = button.map(Button::new);
    let bw = button.as_ref().map_or(0.0, |b| b.width(ui));
    let text_x = m::OFF_PAD[0] + glyph + m::OFF_GAP;
    let tail = if bw > 0.0 {
        bw + m::OFF_GAP + m::OFF_PAD[1]
    } else {
        m::OFF_PAD[0]
    };
    let text_w = (w - text_x - tail).max(1.0);
    let g = Type::Body.galley(ui, text, p.t2, text_w, usize::MAX);
    let h = (g.size().y + 2.0 * m::OFF_PAD[1]).max(m::OFF_MIN_H);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    crate::a11y::item(&resp, egui::Role::Label, text);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(m::RADIUS), p.o1);
    if ui.ctx().theme() == egui::Theme::Dark && p.kind == theme::palette::ThemeKind::HighContrast
        || p.kind == theme::palette::ThemeKind::HighContrast
    {
        painter.rect_stroke(
            rect,
            CornerRadius::same(m::RADIUS),
            Stroke::new(stroke::BORDER, p.line),
            StrokeKind::Inside,
        );
    }
    painter.text(
        pos2(
            at(rect, m::OFF_PAD[0], glyph, rtl) + glyph / 2.0,
            rect.center().y,
        ),
        Align2::CENTER_CENTER,
        icon.text(false),
        icons::font(glyph),
        p.t2,
    );
    let gx = if rtl {
        rect.right() - text_x - g.size().x
    } else {
        rect.left() + text_x
    };
    painter.galley(pos2(gx, rect.center().y - g.size().y / 2.0), g, p.t2);
    let Some(button) = button else {
        return false;
    };
    let bh = theme::tokens::control::BUTTON;
    let br = Rect::from_min_size(
        pos2(
            at(rect, w - m::OFF_PAD[1] - bw, bw, rtl),
            rect.center().y - bh / 2.0,
        ),
        vec2(bw, bh),
    );
    ui.put(br, button).clicked()
}

/// "You blocked Rex. …" with Unblock (when unblocking is carried).
fn blocked_bar(ui: &mut Ui, env: &mut Env<'_>, p: &Palette, friend: &Friend, can: bool, rtl: bool) {
    let text = s::blocked(first(&friend.name));
    if plate(ui, p, icons::PROHIBIT, &text, can.then(s::UNBLOCK), rtl) {
        env.bridge.send(UiCommand::Chat(ChatCommand::SetBlocked {
            friend: friend.id,
            on: false,
        }));
    }
}

/// Why the field is closed (joining a group, a friend not connected yet…).
fn closed_bar(ui: &mut Ui, p: &Palette, why: &str, rtl: bool) {
    plate(ui, p, icons::INFO, why, None, rtl);
}

/// `.cmp-net`: the amber line above the field.
fn offline_line(ui: &mut Ui, p: &Palette, rtl: bool) {
    let w = ui.available_width();
    let text_x = m::NET_ICON + m::NET_GAP;
    let g = Type::Caption.galley(ui, s::OFFLINE(), p.amber, w - text_x, usize::MAX);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    crate::a11y::item(&resp, egui::Role::Label, s::OFFLINE());
    let painter = ui.painter();
    let line = Type::Caption.line_height();
    painter.text(
        pos2(
            at(rect, 0.0, m::NET_ICON, rtl) + m::NET_ICON / 2.0,
            rect.top() + line / 2.0,
        ),
        Align2::CENTER_CENTER,
        icons::WARNING.text(false),
        icons::font(m::NET_ICON),
        p.amber,
    );
    let gx = if rtl {
        rect.right() - text_x - g.size().x
    } else {
        rect.left() + text_x
    };
    painter.galley(pos2(gx, rect.top()), g, p.amber);
    ui.add_space(m::NET_BELOW);
}

// ---- Round buttons --------------------------------------------------------------------

/// How a round button looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// t3, o2 under the pointer.
    Plain,
    /// Its menu or popover is open, or it is on: o2 and t1.
    On,
    /// The filled Send.
    Send,
    /// Send with nothing to send.
    SendOff,
}

/// What a round button shows.
#[derive(Clone, Copy)]
enum Face {
    Glyph(Icon),
    /// "Aa".
    Aa,
    /// The boxed "GIF".
    Gif,
}

/// `.cb`: a round 32 button at `rect`.
#[allow(clippy::too_many_arguments)]
fn round(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    face: Face,
    look: Look,
    tip: &str,
    spoken: &str,
    shortcut: Option<&str>,
) -> Response {
    let sense = if look == Look::SendOff {
        Sense::hover()
    } else {
        Sense::click()
    };
    let resp = ui.interact(rect, id, sense);
    let p = theme::theme(ui.ctx()).palette.clone();
    let reduced = theme::theme(ui.ctx()).motion.reduced;
    let hot = resp.hovered() || resp.has_focus();
    let (fill, fg) = match look {
        Look::Send if hot => (p.fill_hover, p.on_fill),
        Look::Send => (p.fill, p.on_fill),
        Look::SendOff => (p.o1, p.t4),
        Look::On => (p.o2, p.t1),
        Look::Plain if hot => (p.o2, p.t1),
        Look::Plain => (Color32::TRANSPARENT, p.t3),
    };
    let r = if resp.is_pointer_button_down_on() && !reduced {
        motion::scaled(rect, 0.94)
    } else {
        rect
    };
    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.circle_filled(r.center(), r.width() / 2.0, fill);
    }
    match face {
        Face::Glyph(icon) => {
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                icon.text(matches!(look, Look::Send)),
                icons::font(icon_size::INLINE),
                fg,
            );
        }
        Face::Aa => {
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                "Aa",
                Type::BodyBold.font_id(),
                fg,
            );
        }
        Face::Gif => {
            let font = FontId::new(m::GIF_SIZE, Type::BodyBold.font_id().family);
            let g = painter.layout_no_wrap("GIF".into(), font, fg);
            let b = Rect::from_center_size(
                r.center(),
                g.size() + 2.0 * vec2(m::GIF_PAD[0], m::GIF_PAD[1]),
            );
            painter.rect_stroke(
                b,
                CornerRadius::same(m::GIF_RADIUS),
                Stroke::new(stroke::FIELD_FOCUS, fg),
                StrokeKind::Inside,
            );
            painter.galley(b.center() - g.size() / 2.0, g, fg);
        }
    }
    if resp.has_focus() {
        painter.circle_stroke(
            rect.center(),
            rect.width() / 2.0 + stroke::FOCUS_GAP,
            Stroke::new(stroke::FOCUS, p.accent_mark),
        );
    }
    crate::a11y::button(&resp, spoken);
    tooltip::tooltip(resp, tip, shortcut)
}

// ---- The recording --------------------------------------------------------------------

/// A bar's height (0–1) of the recording's waveform: steady per bar.
fn wave_bar(k: usize) -> f32 {
    let mut x = (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    let r = (x % 1000) as f32 / 1000.0;
    let s = (k as f64 * m::REC_TICK * 3.0).sin().abs() as f32;
    0.15 + 0.85 * r * (0.4 + 0.6 * s)
}

/// "0:07".
fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// `.rec-row` inside the field: dot · time · waveform · "5 min at most" · discard · send.
fn recording(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    chat: ChatRef,
    p: &Palette,
    rtl: bool,
) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let t0 = state.chat.recording.unwrap_or(now);
    let elapsed = (now - t0).max(0.0);
    let reduced = theme::theme(&ctx).motion.reduced;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, m::REC_H), Sense::hover());
    crate::a11y::item(&resp, egui::Role::Label, s::RECORDING());
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        CornerRadius::same(m::RADIUS),
        p.well,
        Stroke::new(stroke::BORDER, p.line),
        StrokeKind::Inside,
    );
    let cy = rect.center().y;
    let mut x = m::REC_PAD[0];
    // The dot pulses (not under reduced motion).
    let alpha = if reduced {
        1.0
    } else {
        let phase = (elapsed % m::PULSE_S / m::PULSE_S * std::f64::consts::TAU).cos() as f32;
        m::PULSE_LOW + (1.0 - m::PULSE_LOW) * (phase + 1.0) / 2.0
    };
    painter.circle_filled(
        pos2(at(rect, x, m::REC_DOT, rtl) + m::REC_DOT / 2.0, cy),
        m::REC_DOT / 2.0,
        p.coral.gamma_multiply(alpha),
    );
    x += m::REC_DOT + m::REC_GAP;
    let time = Type::Body.numeric(mmss(elapsed as u32));
    let g = egui::WidgetText::from(time).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    painter.galley(
        pos2(at(rect, x, g.size().x, rtl), cy - g.size().y / 2.0),
        g,
        p.t1,
    );
    x += m::REC_TIME_W + m::REC_GAP;
    // From the end: send, discard, the limit; the waveform fills what is left.
    let mut end = m::REC_PAD[1];
    let send_r = Rect::from_min_size(
        pos2(at(rect, w - end - m::BTN, m::BTN, rtl), cy - m::BTN / 2.0),
        Vec2::splat(m::BTN),
    );
    end += m::BTN + m::ROW_GAP;
    let discard_r = Rect::from_min_size(
        pos2(at(rect, w - end - m::BTN, m::BTN, rtl), cy - m::BTN / 2.0),
        Vec2::splat(m::BTN),
    );
    end += m::BTN + m::REC_GAP;
    let limit = Type::Caption.galley(ui, s::REC_LIMIT(), p.t3, f32::INFINITY, 1);
    let lw = limit.size().x;
    painter.galley(
        pos2(at(rect, w - end - lw, lw, rtl), cy - limit.size().y / 2.0),
        limit,
        p.t3,
    );
    end += lw + m::REC_GAP;
    let wave_w = (w - x - end).max(0.0);
    let n = ((elapsed / m::REC_TICK) as usize).min(m::REC_BARS);
    let first_k = (elapsed / m::REC_TICK) as usize - n;
    for j in 0..n {
        // The newest bar stands at the end; older ones walk towards the time.
        let from_end = (n - 1 - j) as f32 * m::BAR_STEP;
        if from_end + m::BAR_W > wave_w {
            continue;
        }
        let bx = x + wave_w - from_end - m::BAR_W;
        let h = (wave_bar(first_k + j) * m::WAVE_H).max(m::BAR_W);
        let r = Rect::from_min_size(
            pos2(at(rect, bx, m::BAR_W, rtl), cy - h / 2.0),
            vec2(m::BAR_W, h),
        );
        painter.rect_filled(r, CornerRadius::same(1), p.t2);
    }
    let discard = round(
        ui,
        discard_r,
        Id::unique("chat.composer.discard"),
        Face::Glyph(icons::TRASH),
        Look::Plain,
        s::DISCARD(),
        s::DISCARD_SPOKEN(),
        Some("Esc"),
    );
    let send = round(
        ui,
        send_r,
        Id::unique("chat.composer.send_voice"),
        Face::Glyph(icons::ARROW_UP),
        Look::Send,
        s::SEND(),
        s::SEND_VOICE(),
        Some("Enter"),
    );
    let enter = state.chat.picker.is_none() && consume(&ctx, Key::Enter);
    if discard.clicked() {
        state.chat.recording = None;
        state.chat.focus_composer = true;
    } else if send.clicked() || enter || elapsed >= m::REC_MAX_S {
        state.chat.recording = None;
        state.chat.focus_composer = true;
        let secs = (elapsed.round() as u32).clamp(1, m::REC_MAX_S as u32);
        env.bridge
            .send(UiCommand::Chat(ChatCommand::SendVoice { chat, secs }));
    } else {
        ctx.request_repaint_after(std::time::Duration::from_millis(90));
    }
}

// ---- Suggestions --------------------------------------------------------------------------

/// The `:name` or `@name` being typed before the cursor (`chars` before it): its kind, what
/// follows the sign, and the sign's character index.
fn token(before: &[char], mentions: bool) -> Option<(SuggestKind, String, usize)> {
    let mut i = before.len();
    while i > 0 {
        let c = before[i - 1];
        if c.is_alphanumeric() || c == '_' || c == '+' {
            i -= 1;
        } else {
            break;
        }
    }
    let sign = i.checked_sub(1)?;
    let starts = sign == 0 || before[sign - 1].is_whitespace();
    if !starts {
        return None;
    }
    let word: String = before[i..].iter().collect();
    match before[sign] {
        ':' if word.chars().count() >= 2
            && word
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '+') =>
        {
            Some((SuggestKind::Emoji, word, sign))
        }
        '@' if mentions && !word.contains('+') => Some((SuggestKind::Mention, word, sign)),
        _ => None,
    }
}

/// How a member's name is written after `@` (SimpleX's `viewName`): as it is, or in single
/// quotes when it has a space or ends with punctuation (`@Jonas`, `@'Jonas Weber'`).
fn handle(name: &str) -> String {
    let quoted = name.chars().any(char::is_whitespace)
        || name
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_punctuation());
    if quoted {
        format!("'{name}'")
    } else {
        name.to_owned()
    }
}

/// The members `@query` may mean: not the user, not blocked, a word of the name starting
/// with it (any case); at most six.
fn members_for<'a>(members: &'a [GroupMember], query: &str) -> Vec<&'a GroupMember> {
    let q = query.to_lowercase();
    members
        .iter()
        .filter(|x| !x.me && !x.blocked)
        .filter(|x| {
            x.name
                .split_whitespace()
                .any(|w| w.to_lowercase().starts_with(&q))
        })
        .take(m::SUGGEST_MAX)
        .collect()
}

/// The members a message names (SimpleX's mention: `@name` up to a space, its trailing
/// punctuation left out, or `@'two words'`, after a space or at the start): (name, member
/// number), once each.
pub fn mentions_in(text: &str, members: &[GroupMember]) -> Vec<(String, i64)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, i64)> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if c != '@' || (i > 0 && chars[i - 1] != ' ') {
            continue;
        }
        let rest = &chars[i + 1..];
        let name: String = match rest.first() {
            Some('\'') => {
                let inner: String = rest[1..].iter().take_while(|c| **c != '\'').collect();
                if rest.get(1 + inner.chars().count()) != Some(&'\'') {
                    continue;
                }
                inner
            }
            Some(&f) if f > ' ' && f != '#' && f != '@' => {
                let word: String = rest.iter().take_while(|c| !c.is_whitespace()).collect();
                word.trim_end_matches(|c: char| c.is_ascii_punctuation())
                    .to_owned()
            }
            _ => continue,
        };
        if name.is_empty() {
            continue;
        }
        if let Some(mb) = members.iter().find(|x| !x.me && x.name == name)
            && !out.iter().any(|(_, id)| *id == mb.id)
        {
            out.push((name, mb.id));
        }
    }
    out
}

/// One suggestion: what it shows and what it inserts.
struct Hit {
    emoji: Option<&'static str>,
    label: String,
    insert: String,
}

fn hits(state: &UiState, snap: &ChatSnapshot, chat: ChatRef, system: bool) -> Vec<Hit> {
    let Some(sg) = &state.chat.suggest else {
        return Vec::new();
    };
    match sg.kind {
        SuggestKind::Emoji => emoji::suggest(&sg.query, &state.chat.freq, m::SUGGEST_MAX, system)
            .into_iter()
            .map(|(e, n)| Hit {
                emoji: Some(e),
                label: format!(":{n}:"),
                insert: format!("{e} "),
            })
            .collect(),
        SuggestKind::Mention => {
            let ChatRef::Group(g) = chat else {
                return Vec::new();
            };
            let Some(group) = snap.group(g) else {
                return Vec::new();
            };
            members_for(&group.member_list, &sg.query)
                .into_iter()
                .map(|x| Hit {
                    emoji: None,
                    label: format!("@{}", x.name),
                    insert: format!("@{} ", handle(&x.name)),
                })
                .collect()
        }
    }
}

/// Replace the token before the cursor with `insert`.
fn pick(ctx: &Context, state: &mut UiState, chat: ChatRef, hit: &Hit) {
    if let Some(e) = hit.emoji {
        emoji::bump(&mut state.chat.freq, e);
    }
    let id = field_id();
    let draft = state.chat.drafts.entry(chat).or_default();
    let mut st = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let chars: Vec<char> = draft.chars().collect();
    let cursor = st
        .cursor
        .char_range()
        .map_or(chars.len(), |r| r.primary.index.0.min(chars.len()));
    if let Some((_, _, sign)) = token(&chars[..cursor], true) {
        let mut out: String = chars[..sign].iter().collect();
        out.push_str(&hit.insert);
        let at = out.chars().count();
        out.extend(&chars[cursor..]);
        *draft = out;
        st.cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        st.store(ctx, id);
    }
    state.chat.suggest = None;
}

/// `.ac`: the suggestions above the field. Returns the one clicked.
fn suggestions(
    ctx: &Context,
    field: Rect,
    list: &[Hit],
    index: usize,
    p: &Palette,
    rtl: bool,
) -> Option<usize> {
    let font = Type::Body.font_id();
    let widest = list
        .iter()
        .map(|h| {
            ctx.fonts_mut(|f| {
                f.layout_no_wrap(h.label.clone(), font.clone(), p.t2)
                    .size()
                    .x
            }) + h.emoji.map_or(0.0, |_| m::AC_EMOJI + m::ITEM_GAP)
        })
        .fold(0.0, f32::max);
    let w = (widest + m::ITEM_PAD[0] + m::ITEM_PAD[1] + 2.0 * m::AC_PAD).max(m::AC_MIN_W);
    let h = list.len() as f32 * m::ITEM_H + 2.0 * m::AC_PAD;
    let x = if rtl {
        field.right() - m::AC_X - w
    } else {
        field.left() + m::AC_X
    };
    let rect = Rect::from_min_size(pos2(x, field.top() - m::AC_GAP - h), vec2(w, h));
    let mut clicked = None;
    egui::Area::new(Id::unique("chat.composer.suggest"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            let painter = ui.painter();
            painter.add(
                p.shade_pop
                    .shadow()
                    .as_shape(rect, CornerRadius::same(m::AC_RADIUS)),
            );
            painter.rect(
                rect,
                CornerRadius::same(m::AC_RADIUS),
                p.raised,
                Stroke::new(stroke::BORDER, p.line),
                StrokeKind::Inside,
            );
            let label = if list.first().is_some_and(|h| h.emoji.is_some()) {
                s::EMOJI()
            } else {
                s::MEMBERS()
            };
            crate::a11y::labelled(
                ui.ctx(),
                Id::unique("chat.composer.suggest"),
                egui::Role::ListBox,
                label,
            );
            for (k, hit) in list.iter().enumerate() {
                let r = Rect::from_min_size(
                    pos2(
                        rect.left() + m::AC_PAD,
                        rect.top() + m::AC_PAD + k as f32 * m::ITEM_H,
                    ),
                    vec2(w - 2.0 * m::AC_PAD, m::ITEM_H),
                );
                let resp = ui.interact(r, Id::unique(("chat.composer.suggest", k)), Sense::click());
                if k == index || resp.hovered() {
                    ui.painter()
                        .rect_filled(r, CornerRadius::same(m::ITEM_RADIUS), p.o2);
                }
                let mut x = m::ITEM_PAD[0];
                if let Some(e) = hit.emoji {
                    let er = Rect::from_center_size(
                        pos2(at(r, x, m::AC_EMOJI, rtl) + m::AC_EMOJI / 2.0, r.center().y),
                        Vec2::splat(m::AC_EMOJI),
                    );
                    emoji::draw(ui.painter(), er, e, p.t1);
                    x += m::AC_EMOJI + m::ITEM_GAP;
                }
                let color = if hit.emoji.is_some() { p.t2 } else { p.t1 };
                ui.painter().text(
                    pos2(if rtl { r.right() - x } else { r.left() + x }, r.center().y),
                    if rtl {
                        Align2::RIGHT_CENTER
                    } else {
                        Align2::LEFT_CENTER
                    },
                    i18n::bidi::visual_line(&hit.label, None),
                    font.clone(),
                    color,
                );
                crate::a11y::option(&resp, egui::Role::ListBoxOption, &hit.label, k == index);
                if resp.clicked() {
                    clicked = Some(k);
                }
            }
        });
    clicked
}

// ---- Formatting -------------------------------------------------------------------------

/// A mark of SimpleX Chat's own formatting (`Simplex.Chat.Markdown`, v7.0.3): `*bold*`,
/// `_italic_`, `~strike~`, `` `code` ``, `#secret#` and `!1 coloured!` (1 red · 2 green ·
/// 3 blue · 4 yellow · 5 cyan · 6 magenta). Nothing else is formatted by the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// `*text*`.
    Bold,
    /// `_text_`.
    Italic,
    /// `~text~`.
    Strike,
    /// `` `text` ``.
    Code,
    /// `#text#`: hidden until clicked.
    Secret,
    /// `!n text!`, n in 1..=6.
    Color(u8),
}

impl Mark {
    /// What opens it.
    pub fn open(self) -> String {
        match self {
            Mark::Bold => "*".into(),
            Mark::Italic => "_".into(),
            Mark::Strike => "~".into(),
            Mark::Code => "`".into(),
            Mark::Secret => "#".into(),
            Mark::Color(n) => format!("!{n} "),
        }
    }

    /// What closes it.
    pub fn close(self) -> &'static str {
        match self {
            Mark::Bold => "*",
            Mark::Italic => "_",
            Mark::Strike => "~",
            Mark::Code => "`",
            Mark::Secret => "#",
            Mark::Color(_) => "!",
        }
    }
}

/// The six colours of `!n text!` (1 red · 2 green · 3 blue · 4 yellow · 5 cyan · 6 magenta),
/// readable on the theme's planes. The conversation draws coloured text with these.
pub fn mark_color(kind: theme::palette::ThemeKind, n: u8) -> Color32 {
    let light = kind == theme::palette::ThemeKind::Light;
    let (dark, on_light) = match n {
        1 => (0xFF7A7A, 0xC23B3B),
        2 => (0x8CDCAA, 0x2F8A5B),
        3 => (0x7EC8FF, 0x2E7FB8),
        4 => (0xE6D678, 0x857A1E),
        5 => (0x6FD8E0, 0x1C8790),
        _ => (0xE59BFF, 0xA445C2),
    };
    let rgb: u32 = if light { on_light } else { dark };
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The colours' names, 1 to 6.
fn color_name(n: u8) -> &'static str {
    match n {
        1 => s::COLOR_RED(),
        2 => s::COLOR_GREEN(),
        3 => s::COLOR_BLUE(),
        4 => s::COLOR_YELLOW(),
        5 => s::COLOR_CYAN(),
        _ => s::COLOR_MAGENTA(),
    }
}

/// Wrap the selection `a..b` (characters) of `text` in `mark`; returns the text and where
/// the cursor goes (inside the marks when nothing was selected, after them otherwise). The
/// engine's rules: the text inside starts and ends with something other than a space (the
/// selection's own spaces stay outside the marks), and a mark opens only at the start or
/// after a space (one is put before it when needed).
fn apply_format(text: &str, a: usize, b: usize, mark: Mark) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (a, b) = (
        a.min(chars.len()),
        b.min(chars.len()).max(a.min(chars.len())),
    );
    let sel = &chars[a..b];
    let lead = sel.iter().take_while(|c| c.is_whitespace()).count();
    let trail = sel[lead..]
        .iter()
        .rev()
        .take_while(|c| c.is_whitespace())
        .count();
    let core: String = sel[lead..sel.len() - trail].iter().collect();
    let mut out = String::new();
    let before_is_space = a == 0 || chars[a - 1] == ' ';
    if lead == 0 && !before_is_space {
        out.push(' ');
    } else {
        out.extend(&sel[..lead]);
    }
    out.push_str(&mark.open());
    let inside = a + out.chars().count();
    out.push_str(&core);
    out.push_str(mark.close());
    let after = a + out.chars().count();
    out.extend(&sel[sel.len() - trail..]);
    let cursor = if core.is_empty() { inside } else { after };
    let mut result: String = chars[..a].iter().collect();
    result.push_str(&out);
    result.extend(&chars[b..]);
    (result, cursor)
}

/// Apply a formatting mark at the field's selection.
fn format(ctx: &Context, state: &mut UiState, chat: ChatRef, mark: Mark) {
    let id = field_id();
    let draft = state.chat.drafts.entry(chat).or_default();
    let mut st = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let len = draft.chars().count();
    let (a, b) = st.cursor.char_range().map_or((len, len), |r| {
        let [x, y] = r.sorted_cursors();
        (x.index.0, y.index.0)
    });
    let (text, cursor) = apply_format(draft, a, b, mark);
    *draft = text;
    st.cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(cursor))));
    st.store(ctx, id);
    ctx.memory_mut(|mem| mem.request_focus(id));
}

/// A formatting button: its mark, label and shortcut.
type Format = (Mark, fn() -> &'static str, Option<fn() -> &'static str>);

/// The formatting row's buttons before the colour.
const FORMATS: [Format; 5] = [
    (Mark::Bold, s::BOLD, Some(s::KEY_BOLD)),
    (Mark::Italic, s::ITALIC, Some(s::KEY_ITALIC)),
    (Mark::Strike, s::STRIKE, None),
    (Mark::Code, s::CODE, Some(s::KEY_CODE)),
    (Mark::Secret, s::SPOILER, None),
];

/// `.fmt`: B · I · S · code · secret · colour (a menu of the six), and the hint.
fn format_row(ui: &mut Ui, p: &Palette, rtl: bool) -> Option<Mark> {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, m::FMT_PAD[0] + m::FMT_BTN), Sense::hover());
    let top = rect.top() + m::FMT_PAD[0];
    let mut out = None;
    let mut x = m::FMT_PAD[1];
    let mut button = |ui: &mut Ui, k: usize, label: &str, key: Option<&str>| {
        let r = Rect::from_min_size(
            pos2(at(rect, x, m::FMT_BTN, rtl), top),
            Vec2::splat(m::FMT_BTN),
        );
        x += m::FMT_BTN + m::FMT_GAP;
        let resp = ui.interact(r, Id::unique(("chat.composer.fmt", k)), Sense::click());
        let hot = resp.hovered() || resp.has_focus();
        if hot {
            ui.painter()
                .rect_filled(r, CornerRadius::same(m::FMT_RADIUS), p.o2);
        }
        crate::a11y::button(&resp, label);
        (
            tooltip::tooltip(resp, label, key),
            if hot { p.t1 } else { p.t3 },
        )
    };
    for (k, (mark, label, key)) in FORMATS.iter().enumerate() {
        let (resp, fg) = button(ui, k, label(), key.map(|k| k()));
        let c = resp.rect.center();
        match k {
            0..=2 => {
                let letter = ["B", "I", "S"][k];
                let mut fmt = TextFormat {
                    font_id: if k == 0 {
                        Type::BodyBold.font_id()
                    } else {
                        FontId::new(if k == 1 { 15.0 } else { 13.0 }, FontFamily::Proportional)
                    },
                    color: fg,
                    italics: k == 1,
                    ..TextFormat::default()
                };
                if k == 2 {
                    fmt.strikethrough = Stroke::new(stroke::BORDER, fg);
                }
                let g = ui
                    .painter()
                    .layout_job(LayoutJob::single_section(letter.to_owned(), fmt));
                ui.painter().galley(c - g.size() / 2.0, g, fg);
            }
            _ => {
                let icon = if k == 3 { icons::CODE } else { icons::EYE_OFF };
                ui.painter().text(
                    c,
                    Align2::CENTER_CENTER,
                    icon.text(false),
                    icons::font(m::FMT_ICON),
                    fg,
                );
            }
        }
        if resp.clicked() {
            out = Some(*mark);
        }
    }
    // The colour: six dots in a ring; a menu of the six.
    let (resp, _) = button(ui, FORMATS.len(), s::COLOR(), None);
    let c = resp.rect.center();
    for n in 1..=6u8 {
        let angle =
            (f32::from(n) - 1.0) * std::f32::consts::TAU / 6.0 - std::f32::consts::FRAC_PI_2;
        let at = c + vec2(angle.cos(), angle.sin()) * m::COLOR_RING;
        ui.painter()
            .circle_filled(at, m::COLOR_DOT, mark_color(p.kind, n));
    }
    let popup = egui::Popup::menu(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .align(egui::RectAlign::TOP_START);
    context_menu::popup(popup, |ui| {
        for n in 1..=6u8 {
            let color = mark_color(p.kind, n);
            let item =
                context_menu::dot_check_item(ui, color_name(n), false, true, |painter, r, _| {
                    painter.circle_filled(r.center(), m::COLOR_SWATCH, color);
                });
            if item.clicked() {
                out = Some(Mark::Color(n));
            }
        }
    });
    let hint_w = w - x - m::FMT_HINT_PAD;
    if hint_w > 0.0 {
        let g = Type::Micro.galley(ui, s::MARKDOWN_HINT(), p.t3, hint_w, 1);
        let gx = if rtl {
            rect.left() + m::FMT_HINT_PAD
        } else {
            rect.right() - m::FMT_HINT_PAD - g.size().x
        };
        ui.painter()
            .galley(pos2(gx, top + (m::FMT_BTN - g.size().y) / 2.0), g, p.t3);
    }
    out
}

// ---- The reply and edit bar -------------------------------------------------------------

/// The first line of a message as the bar shows it.
fn summary(body: &MessageBody) -> String {
    match body {
        MessageBody::Text(t) => super::list::plain(t),
        // Conversation agent, round 3c: the notice in words (a direct chat's friend unnamed).
        MessageBody::Notice(n) => crate::strings::chat::conversation::notice(n, ""),
        MessageBody::Voice { .. } => ls::VOICE().to_owned(),
        MessageBody::Gif(_) => ls::GIF().to_owned(),
        MessageBody::Image(_) => s::PHOTO().to_owned(),
        MessageBody::Video(_) => s::VIDEO().to_owned(),
        MessageBody::File(m) => m.name.clone(),
        MessageBody::Call(c) => match c.kind {
            easyplay_core::model::CallKind::Ended => ls::CALL_ENDED(),
            easyplay_core::model::CallKind::Missed => ls::MISSED_CALL(),
            easyplay_core::model::CallKind::Declined => ls::CALL_DECLINED(),
        }
        .to_owned(),
        MessageBody::Stream(st) if st.live => ls::sharing(&st.title),
        MessageBody::Stream(st) => ls::shared(&st.title),
        MessageBody::Deleted => ls::DELETED().to_owned(),
    }
}

/// `.cmp-bar`: "Replying to Mira  the first line ×" or "Editing your message ×". Returns
/// whether × was clicked.
fn reply_bar(
    ui: &mut Ui,
    state: &UiState,
    snap: &ChatSnapshot,
    chat: ChatRef,
    p: &Palette,
    rtl: bool,
) -> bool {
    let editing = state.chat.edit.is_some();
    let Some(item) = state.chat.edit.or(state.chat.reply) else {
        return false;
    };
    let msg = state
        .chat
        .open
        .as_ref()
        .filter(|c| c.chat == chat)
        .and_then(|c| c.messages.iter().find(|x| x.id == item));
    let w = ui.available_width();
    let (rect, bar) = ui.allocate_exact_size(vec2(w, m::BAR_H), Sense::hover());
    let mut spoken = String::new();
    let painter = ui.painter().clone();
    painter.hline(
        rect.x_range(),
        rect.bottom() - stroke::BORDER / 2.0,
        Stroke::new(stroke::BORDER, p.line),
    );
    let cy = rect.center().y;
    let mut x = m::BAR_PAD[0];
    painter.text(
        pos2(at(rect, x, m::BAR_ICON, rtl) + m::BAR_ICON / 2.0, cy),
        Align2::CENTER_CENTER,
        if editing { icons::PENCIL } else { icons::REPLY }.text(false),
        icons::font(m::BAR_ICON),
        p.t3,
    );
    x += m::BAR_ICON + m::BAR_GAP;
    let strong = FontId::new(Type::Caption.size(), Type::BodyBold.font_id().family);
    let piece = |x: &mut f32, text: &str, font: FontId, color: Color32| {
        let g = painter.layout_no_wrap(
            i18n::bidi::visual_line(text, None).into_owned(),
            font,
            color,
        );
        let gw = g.size().x;
        painter.galley(pos2(at(rect, *x, gw, rtl), cy - g.size().y / 2.0), g, color);
        *x += gw;
    };
    if editing {
        piece(&mut x, s::EDITING(), strong.clone(), p.t1);
        spoken.push_str(s::EDITING());
    } else {
        piece(&mut x, s::REPLYING(), Type::Caption.font_id(), p.t3);
        x += 4.0;
        let who = match msg.map(|x| &x.author) {
            Some(MessageAuthor::Me) => s::YOURSELF().to_owned(),
            Some(MessageAuthor::Member(n)) => first(n).to_owned(),
            Some(MessageAuthor::Friend) | None => snap
                .chat_name(chat)
                .map(|n| first(n).to_owned())
                .unwrap_or_default(),
        };
        piece(&mut x, &who, strong, p.t1);
        spoken = format!("{} {who}", s::REPLYING());
    }
    x += m::BAR_GAP;
    let close_x = w - m::BAR_PAD[1] - m::BAR_CLOSE;
    if let Some(msg) = msg {
        spoken = format!("{spoken}: {}", summary(&msg.body));
        let room = close_x - m::BAR_GAP - x;
        if room > 0.0 {
            let g = Type::Caption.galley(ui, &summary(&msg.body), p.t3, room, 1);
            let gw = g.size().x.min(room);
            painter
                .with_clip_rect(Rect::from_min_size(
                    pos2(at(rect, x, room, rtl), rect.top()),
                    vec2(room, m::BAR_H),
                ))
                .galley(pos2(at(rect, x, gw, rtl), cy - g.size().y / 2.0), g, p.t3);
        }
    }
    crate::a11y::item(&bar, egui::Role::Label, &spoken);
    let cr = Rect::from_center_size(
        pos2(
            at(rect, close_x, m::BAR_CLOSE, rtl) + m::BAR_CLOSE / 2.0,
            cy,
        ),
        Vec2::splat(m::BAR_CLOSE),
    );
    let resp = ui.interact(cr, Id::unique("chat.composer.bar_close"), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        painter.circle_filled(cr.center(), m::BAR_CLOSE / 2.0, p.o2);
    }
    painter.text(
        cr.center(),
        Align2::CENTER_CENTER,
        icons::CLOSE.text(false),
        icons::font(icon_size::SMALL),
        if hot { p.t1 } else { p.t3 },
    );
    crate::a11y::button(&resp, s::CANCEL());
    tooltip::tooltip(resp, s::CANCEL(), Some("Esc")).clicked()
}

// ---- Attachments --------------------------------------------------------------------------

/// The desktop's clipboard (Wayland and X11) through `arboard`.
struct SystemClipboard;

impl ClipboardSource for SystemClipboard {
    fn read(&self) -> Clip {
        let Ok(mut board) = arboard::Clipboard::new() else {
            return Clip::Other;
        };
        if let Ok(files) = board.get().file_list()
            && !files.is_empty()
        {
            return Clip::Files(files);
        }
        match board.get_image() {
            Ok(img) => Clip::Image {
                size: [img.width, img.height],
                rgba: img.bytes.into_owned(),
            },
            Err(_) => Clip::Other,
        }
    }
}

/// A paste this pass (`Event::Paste`, or `Event::PasteImage` when the clipboard has no
/// text): copied files or a picture join the attachments instead of going into the text.
fn take_paste(ctx: &Context, state: &mut UiState) {
    let event_image = ctx.input(|i| {
        let mut pasted = false;
        let mut image = None;
        for e in &i.events {
            match e {
                Event::Paste(_) => pasted = true,
                Event::PasteImage(img) => {
                    pasted = true;
                    image = Some(img.clone());
                }
                _ => {}
            }
        }
        pasted.then_some(image)
    });
    let Some(event_image) = event_image else {
        return;
    };
    let source = state.chat.clipboard.source();
    let held: Vec<Attachment> = match source.map_or_else(|| SystemClipboard.read(), |c| c.read()) {
        Clip::Files(files) if !files.is_empty() => {
            files.into_iter().map(Attachment::File).collect()
        }
        Clip::Image { size, rgba } if rgba.len() == size[0] * size[1] * 4 => {
            vec![Attachment::Image {
                size,
                rgba: std::sync::Arc::new(rgba),
            }]
        }
        _ => event_image
            .map(|img| Attachment::Image {
                size: img.size,
                rgba: std::sync::Arc::new(
                    img.pixels
                        .iter()
                        .flat_map(|px| px.to_srgba_unmultiplied())
                        .collect(),
                ),
            })
            .into_iter()
            .collect(),
    };
    if !held.is_empty() {
        state.chat.attach(held);
        ctx.input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, Event::Paste(_) | Event::PasteImage(_)));
        });
    }
}

/// Messages built off the UI thread (pictures encoded to PNG), waiting to go out in order.
#[derive(Clone, Default)]
struct Encoded(std::sync::Arc<std::sync::Mutex<Vec<ChatCommand>>>);

impl Encoded {
    fn take(&self) -> Vec<ChatCommand> {
        std::mem::take(
            &mut *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

fn encoded(ctx: &Context) -> Encoded {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Encoded>(Id::unique("chat.composer.encoded"))
            .clone()
    })
}

/// A picture as PNG bytes (fast compression: it is a screenshot on its way out).
fn encode_png(size: [usize; 2], rgba: &[u8]) -> Option<Vec<u8>> {
    use image::ImageEncoder as _;
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        &mut out,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    );
    encoder
        .write_image(
            rgba,
            u32::try_from(size[0]).ok()?,
            u32::try_from(size[1]).ok()?,
            image::ExtendedColorType::Rgba8,
        )
        .ok()?;
    Some(out)
}

/// The commands that send `items` with `caption` on the first (pictures encoded to PNG).
fn attachment_commands(chat: ChatRef, items: Vec<Attachment>, caption: &str) -> Vec<ChatCommand> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(k, a)| {
            let text = if k == 0 {
                caption.to_owned()
            } else {
                String::new()
            };
            match a {
                Attachment::File(path) => Some(ChatCommand::SendFile { chat, path, text }),
                Attachment::Image { size, rgba } => {
                    encode_png(size, &rgba).map(|bytes| ChatCommand::SendClipboard {
                        chat,
                        bytes,
                        kind: ClipKind::Png,
                        text,
                    })
                }
            }
        })
        .collect()
}

/// Send the attachments, the draft as the first one's caption. Files go at once; with a
/// picture among them, the whole batch is built on a worker thread (encoding) and goes out
/// in order when ready (`show` picks it up).
fn send_attachments(
    ctx: &Context,
    state: &mut UiState,
    env: &mut Env<'_>,
    chat: ChatRef,
    caption: String,
) {
    let items = std::mem::take(&mut state.chat.attachments);
    if items.iter().all(|a| matches!(a, Attachment::File(_))) {
        for cmd in attachment_commands(chat, items, &caption) {
            env.bridge.send(UiCommand::Chat(cmd));
        }
        return;
    }
    let out = encoded(ctx);
    let waker = ctx.clone();
    let spawned = std::thread::Builder::new()
        .name("easyplay-attach".into())
        .spawn(move || {
            let cmds = attachment_commands(chat, items, &caption);
            out.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend(cmds);
            waker.request_repaint();
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "chat_attachment_encoder_not_started");
    }
}

/// Files dropped on the window this pass, while this conversation can take them: they join
/// the attachments; folders are skipped, and only folders say so in a toast. The UI does
/// no I/O, so a path without an extension counts as a folder (as the app's own drop routing
/// does). Marks `state.chat.takes_drops` so the shell's drop zone (adding games), which runs
/// after the page, stands aside.
fn take_drop(ctx: &Context, state: &mut UiState, env: &mut Env<'_>) {
    state.chat.takes_drops = true;
    let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
        i.raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect()
    });
    if dropped.is_empty() {
        return;
    }
    ctx.input_mut(|i| i.raw.dropped_files.clear());
    let files: Vec<Attachment> = dropped
        .into_iter()
        .filter(|p| p.extension().is_some())
        .map(Attachment::File)
        .collect();
    if files.is_empty() {
        state.toast(
            s::FOLDERS_REFUSED(),
            crate::vm::toasts::ToastKind::Warning,
            None,
            env.mono,
        );
        return;
    }
    state.chat.attach(files);
    state.chat.focus_composer = true;
}

/// The overlay over the conversation while files are dragged over the window: the wall at
/// 85 %, a dashed 2 pt accent rounded rect inset 16, the file glyph at 32 and "Drop to send
/// to Mira". It fades in over 140 (at once under reduced motion).
fn drop_overlay(ctx: &Context, column: Rect, name: &str) {
    if ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let th = theme::theme(ctx);
    let p = th.palette.clone();
    let id = Id::unique("chat.composer.drop");
    let shown = if th.motion.reduced {
        1.0
    } else {
        motion::appear(ctx, id.with("in"), theme::Dur::Fast.secs())
    };
    egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(column.min)
        .interactable(false)
        .show(ctx, |ui| {
            ui.multiply_opacity(shown);
            let painter = ui.painter();
            painter.rect_filled(
                column,
                CornerRadius::ZERO,
                p.bg.gamma_multiply(m::DROP_WALL),
            );
            let inner = column.shrink(m::DROP_INSET);
            painter.extend(egui::Shape::dashed_line(
                &rounded_outline(inner, m::RADIUS.into()),
                Stroke::new(stroke::FOCUS, p.accent_mark),
                m::DROP_DASH[0],
                m::DROP_DASH[1],
            ));
            let label = s::drop_to(name);
            let g = Type::Heading.galley(ui, &label, p.t1, inner.width() - 2.0 * m::DROP_INSET, 2);
            let h = m::DROP_ICON + m::DROP_GAP + g.size().y;
            let c = inner.center();
            let top = c.y - h / 2.0;
            painter.text(
                pos2(c.x, top + m::DROP_ICON / 2.0),
                Align2::CENTER_CENTER,
                icons::FILE.text(false),
                icons::font(m::DROP_ICON),
                p.accent_text,
            );
            painter.galley(
                pos2(c.x - g.size().x / 2.0, top + m::DROP_ICON + m::DROP_GAP),
                g,
                p.t1,
            );
            crate::a11y::labelled(ui.ctx(), id, egui::Role::Label, &label);
        });
}

/// The outline of `rect` with corners of `radius`, as points for a dashed line.
fn rounded_outline(rect: Rect, radius: f32) -> Vec<egui::Pos2> {
    use std::f32::consts::{FRAC_PI_2, PI};
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -FRAC_PI_2),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), FRAC_PI_2),
        (pos2(rect.left() + r, rect.top() + r), PI),
    ];
    let steps = 8;
    let mut pts = vec![pos2(rect.left() + r, rect.top())];
    for (c, start) in corners {
        for k in 0..=steps {
            let a = start + FRAC_PI_2 * k as f32 / steps as f32;
            pts.push(c + vec2(a.cos(), a.sin()) * r);
        }
    }
    pts.push(pos2(rect.left() + r, rect.top()));
    pts
}

/// What a file is, by its extension.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Picture,
    Video,
    Other,
}

fn file_kind(path: &std::path::Path) -> FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => FileKind::Picture,
        "mp4" | "webm" | "mkv" | "mov" | "m4v" | "avi" => FileKind::Video,
        _ => FileKind::Other,
    }
}

/// What a worker thread found out about a file: its size, and a picture's thumbnail.
#[derive(Clone, Default)]
struct FileMeta {
    size: Option<u64>,
    image: Option<egui::ColorImage>,
}

type Metas = std::sync::Arc<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Option<FileMeta>>>,
>;

/// A file's size and (for a picture) thumbnail texture, found on a worker thread the first
/// time (the UI does no I/O); nothing until then.
fn file_meta(
    ctx: &Context,
    path: &std::path::Path,
) -> (Option<u64>, Option<(egui::TextureId, Vec2)>) {
    let metas: Metas = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Metas>(Id::unique("chat.composer.file_metas"))
            .clone()
    });
    let mut map = metas
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match map.get(path) {
        Some(Some(meta)) => {
            let tex_key = Id::unique(("chat.composer.file_thumb", path));
            let tex = meta.image.as_ref().map(|image| {
                let t = ctx
                    .data(|d| d.get_temp::<egui::TextureHandle>(tex_key))
                    .unwrap_or_else(|| {
                        let t = ctx.load_texture(
                            "chat-attachment",
                            image.clone(),
                            egui::TextureOptions::LINEAR,
                        );
                        ctx.data_mut(|d| d.insert_temp(tex_key, t.clone()));
                        t
                    });
                (t.id(), t.size_vec2())
            });
            (meta.size, tex)
        }
        Some(None) => (None, None),
        None => {
            map.insert(path.to_path_buf(), None);
            let (owned, store, waker) = (path.to_path_buf(), metas.clone(), ctx.clone());
            let picture = file_kind(path) == FileKind::Picture;
            let side = (m::TILE * 2.0) as u32;
            let _ = std::thread::Builder::new()
                .name("easyplay-attach-meta".into())
                .spawn(move || {
                    let size = std::fs::metadata(&owned).ok().map(|md| md.len());
                    let image = if picture {
                        image::open(&owned).ok().map(|img| {
                            let small = img.thumbnail(side, side).to_rgba8();
                            egui::ColorImage::from_rgba_unmultiplied(
                                [small.width() as usize, small.height() as usize],
                                small.as_raw(),
                            )
                        })
                    } else {
                        None
                    };
                    store
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .insert(owned, Some(FileMeta { size, image }));
                    waker.request_repaint();
                });
            (None, None)
        }
    }
}

/// The texture of a pasted picture (uploaded once per paste).
fn pasted_texture(
    ctx: &Context,
    size: [usize; 2],
    rgba: &std::sync::Arc<Vec<u8>>,
) -> (egui::TextureId, Vec2) {
    let key = Id::unique((
        "chat.composer.paste_tex",
        std::sync::Arc::as_ptr(rgba) as usize,
    ));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return (t.id(), t.size_vec2());
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba);
    let t = ctx.load_texture("chat-paste", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, t.clone()));
    (t.id(), t.size_vec2())
}

/// An attachment's name (its file name, or "Picture from the clipboard").
fn attachment_name(a: &Attachment) -> String {
    match a {
        Attachment::Image { .. } => s::PASTED_PICTURE().to_owned(),
        Attachment::File(path) => path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
    }
}

/// A stable id for an attachment's tile (its motion and its ×).
fn attachment_id(a: &Attachment) -> Id {
    match a {
        Attachment::Image { rgba, .. } => {
            Id::unique(("chat.composer.tile", std::sync::Arc::as_ptr(rgba) as usize))
        }
        Attachment::File(path) => Id::unique(("chat.composer.tile", path)),
    }
}

/// The UV rect that crops a picture of `size` to a centred square (cover).
fn cover_uv(size: Vec2) -> Rect {
    let (w, h) = (size.x.max(1.0), size.y.max(1.0));
    if w > h {
        let d = (1.0 - h / w) / 2.0;
        Rect::from_min_max(pos2(d, 0.0), pos2(1.0 - d, 1.0))
    } else {
        let d = (1.0 - w / h) / 2.0;
        Rect::from_min_max(pos2(0.0, d), pos2(1.0, 1.0 - d))
    }
}

/// The attachment strip inside the composer, above the text line: an 80 pt tile for a
/// picture or video (cover-cropped, radius 10; a video has a play glyph), a 200 × 56 chip
/// for any other file (an icon tile, the name and size), each with a 20 pt round × at its
/// top-right corner, hanging 6 over. 12 padding, 8 apart; scrolls sideways when wide.
/// Tiles pop in (scale .9 → 1) and slide to their places when one goes. Returns the index
/// of the attachment whose × was clicked.
fn attachment_strip(ui: &mut Ui, items: &[Attachment], p: &Palette, rtl: bool) -> Option<usize> {
    let ctx = ui.ctx().clone();
    let th = theme::theme(&ctx);
    let reduced = th.motion.reduced;
    let mut removed = None;
    let widths: Vec<f32> = items
        .iter()
        .map(|a| match a {
            Attachment::File(path) if file_kind(path) == FileKind::Other => m::CHIP[0],
            _ => m::TILE,
        })
        .collect();
    let content_w = m::STRIP_PAD * 2.0
        + widths.iter().sum::<f32>()
        + m::TILE_GAP * (widths.len().saturating_sub(1)) as f32;
    let h = m::STRIP_PAD + m::TILE + m::STRIP_BELOW;
    let w = ui.available_width();
    let (strip, strip_resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    crate::a11y::labelled(
        &ctx,
        strip_resp.id,
        egui::Role::List,
        &s::attachments(items.len()),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(strip)
            .layout(egui::Layout::left_to_right(Align::Min)),
    );
    egui::ScrollArea::horizontal()
        .id_salt("chat.composer.strip")
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(&mut child, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(content_w.max(w), h), Sense::hover());
            let painter = ui.painter().clone();
            let mut x = m::STRIP_PAD;
            for (k, (a, tw)) in items.iter().zip(&widths).enumerate() {
                let id = attachment_id(a);
                // Slide to the place (after a removal), whole points.
                let target = x;
                let gx = if reduced {
                    target
                } else {
                    motion::glide(&ctx, id.with("x"), target, theme::Dur::Fast.secs())
                };
                x += tw + m::TILE_GAP;
                let th_h = if *tw == m::TILE { m::TILE } else { m::CHIP[1] };
                let left = if rtl {
                    area.right() - gx - tw
                } else {
                    area.left() + gx
                };
                let tile = Rect::from_min_size(
                    pos2(left.round(), (area.top() + m::STRIP_PAD).round()),
                    vec2(*tw, th_h),
                );
                let t = if reduced {
                    1.0
                } else {
                    motion::ease_out(motion::appear(&ctx, id.with("in"), theme::Dur::Fast.secs()))
                };
                let scale = m::TILE_FROM + (1.0 - m::TILE_FROM) * t;
                let drawn = Rect::from_center_size(tile.center(), tile.size() * scale);
                let mut pt = painter.clone();
                pt.multiply_opacity(t);
                let radius = CornerRadius::same(m::TILE_RADIUS);
                match a {
                    Attachment::Image { size, rgba } => {
                        let (tex, tsize) = pasted_texture(&ctx, *size, rgba);
                        pt.add(
                            egui::epaint::RectShape::filled(drawn, radius, Color32::WHITE)
                                .with_texture(tex, cover_uv(tsize)),
                        );
                    }
                    Attachment::File(path) => match file_kind(path) {
                        FileKind::Other => {
                            pt.rect_filled(drawn, radius, p.o1);
                            let icon_r = Rect::from_min_size(
                                pos2(
                                    if rtl {
                                        drawn.right() - m::CHIP_PAD - m::CHIP_ICON
                                    } else {
                                        drawn.left() + m::CHIP_PAD
                                    },
                                    drawn.center().y - m::CHIP_ICON / 2.0,
                                ),
                                Vec2::splat(m::CHIP_ICON),
                            );
                            pt.rect_filled(icon_r, CornerRadius::same(m::CHIP_ICON_RADIUS), p.o2);
                            pt.text(
                                icon_r.center(),
                                Align2::CENTER_CENTER,
                                icons::FILE.text(false),
                                icons::font(icon_size::INLINE),
                                p.t2,
                            );
                            let text_w =
                                drawn.width() - m::CHIP_PAD * 3.0 - m::CHIP_ICON - m::CORNER_ROOM;
                            let name =
                                Type::BodyStrong.galley(ui, &attachment_name(a), p.t1, text_w, 1);
                            let (bytes, _) = file_meta(&ctx, path);
                            let size_line = bytes.map(|b| {
                                Type::Caption.galley(
                                    ui,
                                    &crate::strings::fmt::bytes(b),
                                    p.t3,
                                    text_w,
                                    1,
                                )
                            });
                            let total =
                                name.size().y + size_line.as_ref().map_or(0.0, |g| g.size().y);
                            let tx = if rtl {
                                icon_r.left() - m::CHIP_PAD
                            } else {
                                icon_r.right() + m::CHIP_PAD
                            };
                            let mut ty = drawn.center().y - total / 2.0;
                            let nx = if rtl { tx - name.size().x } else { tx };
                            pt.galley(pos2(nx, ty), name.clone(), p.t1);
                            ty += name.size().y;
                            if let Some(g) = size_line {
                                let sx = if rtl { tx - g.size().x } else { tx };
                                pt.galley(pos2(sx, ty), g, p.t3);
                            }
                        }
                        kind => {
                            pt.rect_filled(drawn, radius, p.o2);
                            if let (_, Some((tex, tsize))) = file_meta(&ctx, path) {
                                pt.add(
                                    egui::epaint::RectShape::filled(drawn, radius, Color32::WHITE)
                                        .with_texture(tex, cover_uv(tsize)),
                                );
                            }
                            if kind == FileKind::Video {
                                pt.circle_filled(
                                    drawn.center(),
                                    m::PLAY_DISC / 2.0,
                                    Color32::from_black_alpha(150),
                                );
                                pt.text(
                                    drawn.center(),
                                    Align2::CENTER_CENTER,
                                    icons::PLAY.text(true),
                                    icons::font(icon_size::INLINE),
                                    Color32::WHITE,
                                );
                            } else if file_meta(&ctx, path).1.is_none() {
                                pt.text(
                                    drawn.center(),
                                    Align2::CENTER_CENTER,
                                    icons::IMAGE.text(false),
                                    icons::font(icon_size::INLINE),
                                    p.t3,
                                );
                            }
                        }
                    },
                }
                // The ×: a 20 pt disc of the wall's colour hanging 6 over the top-right corner.
                let cx = if rtl {
                    tile.left() - m::CORNER_HANG + m::CORNER / 2.0
                } else {
                    tile.right() + m::CORNER_HANG - m::CORNER / 2.0
                };
                let cr = Rect::from_center_size(
                    pos2(cx, tile.top() - m::CORNER_HANG + m::CORNER / 2.0),
                    Vec2::splat(m::CORNER),
                );
                let resp = ui.interact(cr, id.with("remove"), Sense::click());
                let hot = resp.hovered() || resp.has_focus();
                pt.circle_filled(
                    cr.center(),
                    m::CORNER / 2.0,
                    if hot {
                        theme::palette::over(p.o3, p.bg)
                    } else {
                        p.bg
                    },
                );
                pt.text(
                    cr.center(),
                    Align2::CENTER_CENTER,
                    icons::CLOSE.text(false),
                    icons::font(m::CORNER_GLYPH),
                    p.t1,
                );
                let label = s::remove_named(&attachment_name(a));
                crate::a11y::button(&resp, &label);
                if tooltip::tooltip(resp, &label, None).clicked() {
                    removed = Some(k);
                }
            }
        });
    removed
}

// ---- The field --------------------------------------------------------------------------

/// What the keys asked for in this pass.
enum Act {
    Send,
    Unattach,
    Format(Mark),
    EditLast,
    Move(isize),
    Pick,
    Dismiss,
}

/// The last text message the user sent in the open conversation.
fn last_own(state: &UiState, chat: ChatRef) -> Option<(i64, String)> {
    let c = state.chat.open.as_ref().filter(|c| c.chat == chat)?;
    c.messages
        .iter()
        .rev()
        .find_map(|x| match (&x.author, &x.body) {
            (MessageAuthor::Me, MessageBody::Text(t)) => Some((x.id, t.clone())),
            _ => None,
        })
}

/// Send the draft (or the edit).
fn send(ctx: &Context, state: &mut UiState, env: &mut Env<'_>, snap: &ChatSnapshot, chat: ChatRef) {
    let text = state
        .chat
        .drafts
        .get(&chat)
        .map(|d| d.trim().to_owned())
        .unwrap_or_default();
    // Attachments go with the text as the first one's caption.
    if !state.chat.attachments.is_empty() && state.chat.edit.is_none() {
        send_attachments(ctx, state, env, chat, text);
        state.chat.drafts.entry(chat).or_default().clear();
        state.chat.reply = None;
        state.chat.suggest = None;
        state.chat.focus_composer = true;
        return;
    }
    if text.is_empty() {
        return;
    }
    let f = snap.features;
    if let Some(item) = state.chat.edit.take() {
        let before = last_text(state, chat, item);
        if f.edit && before.as_deref() != Some(text.as_str()) {
            env.bridge.send(UiCommand::Chat(ChatCommand::Edit {
                chat,
                item,
                text: text.clone(),
            }));
        }
    } else {
        let mentions = match chat {
            ChatRef::Group(g) if f.mentions => snap
                .group(g)
                .map(|g| mentions_in(&text, &g.member_list))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        env.bridge.send(UiCommand::Chat(ChatCommand::Send {
            chat,
            text: text.clone(),
            reply_to: state.chat.reply.filter(|_| f.replies),
            mentions,
        }));
    }
    state.chat.drafts.entry(chat).or_default().clear();
    state.chat.reply = None;
    state.chat.suggest = None;
    state.chat.focus_composer = true;
}

fn last_text(state: &UiState, chat: ChatRef, item: i64) -> Option<String> {
    let c = state.chat.open.as_ref().filter(|c| c.chat == chat)?;
    c.messages
        .iter()
        .find(|x| x.id == item)
        .and_then(|x| match &x.body {
            MessageBody::Text(t) => Some(t.clone()),
            _ => None,
        })
}

// ---- Formatting as it is typed --------------------------------------------------------

/// How a byte range of the draft shows in the field.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Live {
    /// Typed text in no format.
    Plain,
    /// A format's marks (`*`, `_`, `!1 `, quotes of a mention): quiet.
    Mark,
    /// Text in a format.
    Kind(super::markdown::Kind),
}

/// The draft's byte ranges and how each shows: the conversation's parser (`markdown::parse`,
/// the engine's own rules) gives the spans as shown; each is found in the line in order, and
/// what lies between them is a format's marks. A span that cannot be found leaves the rest
/// of the line plain.
fn live_ranges(text: &str, mentions: &[String]) -> Vec<(std::ops::Range<usize>, Live)> {
    use super::markdown::{Kind, parse};
    let names: Vec<&str> = mentions.iter().map(String::as_str).collect();
    let lines = parse(text, &names);
    let mut out = Vec::new();
    let mut base = 0;
    for (k, line_text) in text.split('\n').enumerate() {
        let mut at = 0;
        if let Some(spans) = lines.get(k) {
            for sp in spans {
                let rest = &line_text[at..];
                let quoted = sp
                    .text
                    .strip_prefix('@')
                    .map(|n| format!("@'{n}'"))
                    .filter(|_| sp.kind == Kind::Mention);
                let found = match &quoted {
                    Some(q) if rest.find(q.as_str()).is_some() => {
                        rest.find(q.as_str()).map(|i| (i, q.len()))
                    }
                    _ => rest.find(sp.text.as_str()).map(|i| (i, sp.text.len())),
                };
                let Some((i, len)) = found else {
                    break;
                };
                if i > 0 {
                    out.push((base + at..base + at + i, Live::Mark));
                }
                let range = base + at + i..base + at + i + len;
                let look = match sp.kind {
                    Kind::Text | Kind::Edited => Live::Plain,
                    ref k => Live::Kind(k.clone()),
                };
                out.push((range, look));
                at += i + len;
            }
        }
        if at < line_text.len() {
            // What is left (closing marks, or text the parser did not take).
            let rest = &line_text[at..];
            let look = if rest.chars().all(|c| "*_~`#!".contains(c)) {
                Live::Mark
            } else {
                Live::Plain
            };
            out.push((base + at..base + line_text.len(), look));
        }
        base += line_text.len();
        if base < text.len() {
            out.push((base..base + 1, Live::Plain));
            base += 1;
        }
    }
    out
}

/// The field's text as a layout job, formatted as SimpleX will show it. The parse is kept
/// while the text stays the same (the field lays out every pass).
fn live_job(ctx: &Context, text: &str, mentions: &[String], p: &Palette) -> LayoutJob {
    use super::markdown::{Hue, Kind};
    type Cached = (String, Vec<String>, Vec<(std::ops::Range<usize>, Live)>);
    let key = Id::unique("chat.composer.live");
    let ranges = match ctx.data(|d| d.get_temp::<std::sync::Arc<Cached>>(key)) {
        Some(c) if c.0 == text && c.1 == mentions => c.2.clone(),
        _ => {
            let r = live_ranges(text, mentions);
            ctx.data_mut(|d| {
                d.insert_temp(
                    key,
                    std::sync::Arc::new((text.to_owned(), mentions.to_vec(), r.clone())),
                );
            });
            r
        }
    };
    let base = Type::Prose.format(p.t1, false);
    let size = Type::Prose.size();
    let mut job = LayoutJob::default();
    for (range, look) in ranges {
        let mut f = base.clone();
        match &look {
            Live::Plain => {}
            Live::Mark => f.color = p.t3,
            Live::Kind(kind) => match kind {
                Kind::Bold => f.font_id = FontId::new(size, Type::BodyBold.font_id().family),
                Kind::Italic => f.italics = true,
                Kind::Strike => f.strikethrough = Stroke::new(stroke::BORDER, p.t1),
                Kind::Code => {
                    f.font_id = FontId::monospace(m::LIVE_CODE);
                    f.background = p.o2;
                }
                Kind::Secret(_) => f.background = p.o3,
                Kind::Small => {
                    f.font_id = FontId::proportional(m::LIVE_SMALL);
                    f.color = p.t2;
                }
                Kind::Color(h) => {
                    let n = match h {
                        Hue::Red => 1,
                        Hue::Green => 2,
                        Hue::Blue => 3,
                        Hue::Yellow => 4,
                        Hue::Cyan => 5,
                        Hue::Magenta => 6,
                    };
                    f.color = mark_color(p.kind, n);
                }
                Kind::Mention => {
                    f.color = p.accent_text;
                    f.background = p.tint;
                }
                Kind::Link(_) => f.color = p.accent_text,
                Kind::Text | Kind::Edited => {}
            },
        }
        job.append(&text[range], 0.0, f);
    }
    if text.is_empty() {
        job.append("", 0.0, base);
    }
    job
}

/// The field, its bars and buttons, and the suggestions.
#[allow(clippy::too_many_arguments)]
fn field(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    name: &str,
    p: &Palette,
    rtl: bool,
) {
    let ctx = ui.ctx().clone();
    let f: ChatFeatures = snap.features;
    let prefs = env.bridge.prefs().clone();
    let ctrl_send = prefs.chat.send_with == ChatSendWith::CtrlEnter;
    let system = emoji::system(&ctx);
    let id = field_id();
    let group = matches!(chat, ChatRef::Group(_));

    // Focus: asked for (opening a chat, after sending), or typing while nothing has focus.
    // The click that opened the chat (on a row) counts for the field as a click elsewhere,
    // which drops focus: the request stands for this pass and the next.
    let again: u8 = ctx
        .data(|d| d.get_temp(Id::unique("chat.composer.focus_again")))
        .unwrap_or(0);
    if std::mem::take(&mut state.chat.focus_composer) {
        focus_at_end(&ctx, state, chat);
        ctx.data_mut(|d| d.insert_temp(Id::unique("chat.composer.focus_again"), 1u8));
    } else if again > 0 {
        ctx.data_mut(|d| d.insert_temp(Id::unique("chat.composer.focus_again"), again - 1));
        ctx.memory_mut(|mem| mem.request_focus(id));
    } else if ctx.memory(|mem| mem.focused().is_none())
        && state.chat.picker.is_none()
        && state.dialogs.is_empty()
    {
        let typed: String = ctx.input_mut(|i| {
            let mut typed = String::new();
            i.events.retain(|ev| match ev {
                Event::Text(t) if !t.trim().is_empty() && t != "?" && typed.is_empty() => {
                    typed.push_str(t);
                    false
                }
                _ => true,
            });
            typed
        });
        if !typed.is_empty() {
            state.chat.drafts.entry(chat).or_default().push_str(&typed);
            focus_at_end(&ctx, state, chat);
        }
    }

    // The page's Esc (`ChatVm::escape`) takes the suggestions away: they stay away until
    // the token changes.
    let shown: Option<String> = ctx.data(|d| d.get_temp(Id::unique("chat.composer.shown")));
    if let Some(q) = shown
        && state.chat.suggest.is_none()
    {
        ctx.data_mut(|d| d.insert_temp(Id::unique("chat.composer.dismissed"), q));
    }

    // Ctrl+V of a picture or of copied files: they join the attachments.
    let focused = ctx.memory(|mem| mem.has_focus(id));
    if focused && f.files {
        take_paste(&ctx, state);
    }
    let pending = !state.chat.attachments.is_empty();

    // Keys, before the field sees them.
    let list = hits(state, snap, chat, system);
    let open_list = focused && !list.is_empty();
    let empty = state.chat.drafts.get(&chat).is_none_or(String::is_empty);
    let can_edit_last = f.edit && empty && last_own(state, chat).is_some();
    let mut acts: Vec<Act> = Vec::new();
    if focused {
        ctx.input_mut(|i| {
            i.events.retain(|ev| {
                let Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = ev
                else {
                    return true;
                };
                let cmd = modifiers.command || modifiers.ctrl;
                let plain = !cmd && !modifiers.shift && !modifiers.alt;
                // Backspace in an empty field takes the last attachment away.
                if pending && *key == Key::Backspace && plain && empty {
                    acts.push(Act::Unattach);
                    return false;
                }
                if open_list {
                    let act = match key {
                        Key::ArrowDown => Some(Act::Move(1)),
                        Key::ArrowUp => Some(Act::Move(-1)),
                        Key::Enter | Key::Tab => Some(Act::Pick),
                        Key::Escape => Some(Act::Dismiss),
                        _ => None,
                    };
                    if let Some(a) = act {
                        acts.push(a);
                        return false;
                    }
                }
                if *key == Key::Enter && !modifiers.shift && !modifiers.alt && cmd == ctrl_send {
                    acts.push(Act::Send);
                    return false;
                }
                if cmd && !modifiers.shift && !modifiers.alt {
                    let mark = match key {
                        Key::B => Some(Mark::Bold),
                        Key::I => Some(Mark::Italic),
                        Key::E => Some(Mark::Code),
                        _ => None,
                    };
                    if let Some(mark) = mark {
                        acts.push(Act::Format(mark));
                        return false;
                    }
                }
                if *key == Key::ArrowUp && plain && can_edit_last {
                    acts.push(Act::EditLast);
                    return false;
                }
                true
            });
        });
    }
    for act in acts {
        match act {
            Act::Send => send(&ctx, state, env, snap, chat),
            Act::Unattach => {
                state.chat.attachments.pop();
            }
            Act::Format(mark) => format(&ctx, state, chat, mark),
            Act::EditLast => {
                if let Some((item, text)) = last_own(state, chat) {
                    start_edit(state, chat, item, &text);
                    focus_at_end(&ctx, state, chat);
                }
            }
            Act::Move(d) => {
                if let Some(sg) = &mut state.chat.suggest {
                    let n = list.len() as isize;
                    sg.index = (sg.index as isize + d).rem_euclid(n.max(1)) as usize;
                }
            }
            Act::Pick => {
                let k = state.chat.suggest.as_ref().map_or(0, |x| x.index);
                if let Some(hit) = list.get(k) {
                    pick(&ctx, state, chat, hit);
                }
            }
            Act::Dismiss => {
                // Dismissed until the token changes.
                if let Some(sg) = state.chat.suggest.take() {
                    ctx.data_mut(|d| {
                        d.insert_temp(Id::unique("chat.composer.dismissed"), sg.query)
                    });
                }
            }
        }
    }

    // The field's plate.
    let w = ui.available_width();
    let top = ui.cursor().top();
    let plate_left = ui.cursor().left();
    let reply_or_edit = state.chat.edit.is_some() || state.chat.reply.is_some();
    let mut frame_rect = Rect::from_min_size(pos2(plate_left, top), vec2(w, 0.0));
    let bg_idx = ui.painter().add(egui::Shape::Noop);

    let mut close_bar = false;
    if reply_or_edit {
        close_bar = reply_bar(ui, state, snap, chat, p, rtl);
    }
    let mut mark = None;
    if state.chat.format_bar {
        mark = format_row(ui, p, rtl);
    }
    // Pictures and files waiting, above the text line.
    if !state.chat.attachments.is_empty()
        && let Some(k) = attachment_strip(ui, &state.chat.attachments.clone(), p, rtl)
    {
        state.chat.attachments.remove(k);
        state.chat.focus_composer = true;
    }

    // The row: + · text · Aa · GIF · emoji · mic or Send.
    let row_top = ui.cursor().top();
    let draft_empty = state
        .chat
        .drafts
        .get(&chat)
        .is_none_or(|d| d.trim().is_empty());
    let leading = usize::from(f.files);
    let trailing = 3 + usize::from(f.gifs);
    let buttons = (leading + trailing) as f32;
    let text_x = m::ROW_PAD[1] + leading as f32 * (m::BTN + m::ROW_GAP);
    let text_w = (w - 2.0 * m::ROW_PAD[1] - buttons * m::BTN - buttons * m::ROW_GAP).max(m::BTN);
    let text_rect = Rect::from_min_size(
        pos2(at(frame_rect, text_x, text_w, rtl), row_top + m::ROW_PAD[0]),
        vec2(text_w, m::TEXT_MAX),
    );
    let filter = EventFilter {
        tab: open_list,
        horizontal_arrows: true,
        vertical_arrows: true,
        escape: open_list || reply_or_edit || pending,
    };
    let placeholder = if state.chat.attachments.is_empty() {
        s::placeholder(if group { name } else { first(name) })
    } else {
        s::ADD_CAPTION().to_owned()
    };
    let label = s::label(name);
    // SimpleX formatting shows as it is typed (the marks stay, quiet).
    let mentionable: Vec<String> = match chat {
        ChatRef::Group(g) if f.mentions => snap
            .group(g)
            .map(|g| {
                g.member_list
                    .iter()
                    .filter(|x| !x.me)
                    .map(|x| x.name.clone())
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, wrap: f32| {
        let mut job = live_job(ui.ctx(), buf.as_str(), &mentionable, p);
        job.wrap.max_width = wrap;
        job.halign = if rtl { Align::Max } else { Align::Min };
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(egui::Layout::top_down(Align::Min)),
    );
    let draft = state.chat.drafts.entry(chat).or_default();
    let before = draft.clone();
    let out = egui::ScrollArea::vertical()
        .id_salt("chat.composer.scroll")
        .max_height(m::TEXT_MAX)
        .auto_shrink([false, true])
        .show(&mut child, |ui| {
            ui.set_width(text_w);
            egui::TextEdit::multiline(draft)
                .id(id)
                // A frame's margin, not `margin`: egui ignores that when a frame is given.
                .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(m::TEXT_PAD as i8)))
                .desired_rows(1)
                .desired_width(text_w)
                .hint_text(
                    egui::RichText::new(i18n::bidi::visual_line(&placeholder, None))
                        .font(Type::Prose.font_id())
                        .color(p.t3),
                )
                .event_filter(filter)
                .layouter(&mut layouter)
                .show(ui)
        });
    let text_h = out.content_size.y.clamp(m::BTN, m::TEXT_MAX);
    let edit = out.inner;
    edit.response.response.widget_info(|| {
        egui::WidgetInfo::text_edit(true, before.as_str(), draft.as_str(), placeholder.as_str())
    });
    crate::a11y::labelled(&ctx, id, egui::Role::MultilineTextInput, &label);
    let changed = *draft != before;
    let now_focused = edit.response.has_focus();
    let row_h = text_h + 2.0 * m::ROW_PAD[0];
    ui.allocate_exact_size(vec2(w, row_h), Sense::hover());
    let row = Rect::from_min_size(pos2(plate_left, row_top), vec2(w, row_h));
    frame_rect.max.y = row.bottom();

    // Suggestions follow the token before the cursor.
    if now_focused {
        let chars: Vec<char> = draft.chars().collect();
        let cursor = edit
            .cursor_range
            .map_or(chars.len(), |r| r.primary.index.0.min(chars.len()));
        let mention_ok = group && f.mentions;
        let dismissed: Option<String> =
            ctx.data(|d| d.get_temp(Id::unique("chat.composer.dismissed")));
        state.chat.suggest = match token(&chars[..cursor], mention_ok) {
            Some((kind, query, _)) if dismissed.as_deref() != Some(query.as_str()) || changed => {
                let index = state
                    .chat
                    .suggest
                    .as_ref()
                    .filter(|x| x.kind == kind && x.query == query)
                    .map_or(0, |x| x.index);
                Some(Suggest { kind, query, index })
            }
            _ => None,
        };
        if changed {
            ctx.data_mut(|d| d.remove_temp::<String>(Id::unique("chat.composer.dismissed")));
        }
        let list = hits(state, snap, chat, system);
        if list.is_empty() {
            state.chat.suggest = None;
        } else if let Some(sg) = &mut state.chat.suggest {
            sg.index = sg.index.min(list.len() - 1);
        }
        // Tab and Esc belong to the list while it shows (from the next pass's focus keys).
        let open = state.chat.suggest.is_some();
        let q = state.chat.suggest.as_ref().map(|x| x.query.clone());
        ctx.data_mut(|d| match q {
            Some(q) => {
                d.insert_temp(Id::unique("chat.composer.shown"), q);
            }
            None => {
                d.remove::<String>(Id::unique("chat.composer.shown"));
            }
        });
        ctx.memory_mut(|mem| {
            mem.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: open,
                    escape: open || reply_or_edit || !state.chat.attachments.is_empty(),
                    ..filter
                },
            );
        });
    }

    // Buttons, along the row's bottom.
    let by = row.bottom() - m::ROW_PAD[0] - m::BTN;
    let slot = |k: usize| {
        Rect::from_min_size(
            pos2(
                at(
                    row,
                    m::ROW_PAD[1] + k as f32 * (m::BTN + m::ROW_GAP),
                    m::BTN,
                    rtl,
                ),
                by,
            ),
            Vec2::splat(m::BTN),
        )
    };
    let tail = |k: usize| {
        // k-th from the end: 0 is the last.
        Rect::from_min_size(
            pos2(
                at(
                    row,
                    w - m::ROW_PAD[1] - (k + 1) as f32 * m::BTN - k as f32 * m::ROW_GAP,
                    m::BTN,
                    rtl,
                ),
                by,
            ),
            Vec2::splat(m::BTN),
        )
    };
    if f.files {
        plus_menu(ui, state, chat, slot(0));
    }
    let mut k = trailing - 1;
    let aa = round(
        ui,
        tail(k),
        Id::unique("chat.composer.aa"),
        Face::Aa,
        if state.chat.format_bar {
            Look::On
        } else {
            Look::Plain
        },
        s::FORMATTING(),
        s::FORMATTING(),
        None,
    );
    if aa.clicked() {
        state.chat.format_bar = !state.chat.format_bar;
    }
    if f.gifs {
        k -= 1;
        let r = tail(k);
        let open = state.chat.picker == Some(Picker::Gif);
        let gif = round(
            ui,
            r,
            Id::unique("chat.composer.gif"),
            Face::Gif,
            if open { Look::On } else { Look::Plain },
            s::GIF(),
            s::GIF_SPOKEN(),
            None,
        );
        if open {
            emoji::set_anchor(&ctx, r);
        }
        if gif.clicked() {
            state.chat.picker = if open { None } else { Some(Picker::Gif) };
            emoji::set_anchor(&ctx, r);
        }
    }
    k -= 1;
    let er = tail(k);
    let mine = state.chat.picker == Some(Picker::Emoji { react_to: None });
    let em = round(
        ui,
        er,
        Id::unique("chat.composer.emoji"),
        Face::Glyph(icons::SMILEY),
        if mine { Look::On } else { Look::Plain },
        s::EMOJI(),
        s::EMOJI(),
        None,
    );
    if mine {
        emoji::set_anchor(&ctx, er);
    }
    if em.clicked() {
        state.chat.picker = if mine {
            None
        } else {
            Some(Picker::Emoji { react_to: None })
        };
        emoji::set_anchor(&ctx, er);
    }
    let go = tail(0);
    let nothing = draft_empty && state.chat.attachments.is_empty();
    if f.voice && nothing && state.chat.edit.is_none() {
        let mic = round(
            ui,
            go,
            Id::unique("chat.composer.go"),
            Face::Glyph(icons::MIC),
            Look::Plain,
            s::RECORD(),
            s::RECORD(),
            None,
        );
        if mic.clicked() {
            state.chat.recording = Some(ctx.input(|i| i.time));
            state.chat.picker = None;
        }
    } else {
        let key = if ctrl_send {
            s::KEY_SEND_CTRL()
        } else {
            s::KEY_SEND()
        };
        let look = if nothing { Look::SendOff } else { Look::Send };
        if round(
            ui,
            go,
            Id::unique("chat.composer.go"),
            Face::Glyph(icons::ARROW_UP),
            look,
            s::SEND(),
            s::SEND(),
            Some(key),
        )
        .clicked()
        {
            send(&ctx, state, env, snap, chat);
        }
    }

    // The plate under everything, and its line (accent while the field has focus).
    let focused_now = ctx.memory(|mem| mem.has_focus(id));
    ui.painter().set(
        bg_idx,
        egui::Shape::rect_filled(frame_rect, CornerRadius::same(m::RADIUS), p.well),
    );
    let line = if focused_now {
        Stroke::new(stroke::FIELD_FOCUS, p.accent_mark)
    } else {
        Stroke::new(stroke::BORDER, p.line)
    };
    ui.painter().rect_stroke(
        frame_rect,
        CornerRadius::same(m::RADIUS),
        line,
        StrokeKind::Inside,
    );

    // The suggestions above the plate.
    // From the focus at the start of the pass: a click on a suggestion takes the field's
    // focus away before the list would be drawn.
    if (focused || focused_now) && state.chat.suggest.is_some() {
        let list = hits(state, snap, chat, system);
        let index = state.chat.suggest.as_ref().map_or(0, |x| x.index);
        if let Some(k) = suggestions(&ctx, frame_rect, &list, index, p, rtl)
            && let Some(hit) = list.get(k)
        {
            pick(&ctx, state, chat, hit);
            ctx.memory_mut(|mem| mem.request_focus(id));
        }
    }

    if let Some(mark) = mark {
        format(&ctx, state, chat, mark);
    }
    if close_bar {
        cancel_bar(state, chat);
    }
}

/// Give the field focus with the cursor at the end of the draft.
fn focus_at_end(ctx: &Context, state: &mut UiState, chat: ChatRef) {
    let id = field_id();
    let n = state
        .chat
        .drafts
        .get(&chat)
        .map_or(0, |d| d.chars().count());
    let mut st = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    st.cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(n))));
    st.store(ctx, id);
    ctx.memory_mut(|mem| mem.request_focus(id));
}

/// + and its menu: a picture, a file (each at most 1 MB).
fn plus_menu(ui: &mut Ui, state: &mut UiState, chat: ChatRef, rect: Rect) {
    let popup_id = Id::unique("chat.composer.attach.menu");
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let resp = round(
        ui,
        rect,
        Id::unique("chat.composer.attach"),
        Face::Glyph(icons::PLUS),
        if open { Look::On } else { Look::Plain },
        s::ATTACH(),
        s::ATTACH(),
        None,
    );
    let popup = egui::Popup::menu(&resp)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .align(egui::RectAlign::TOP_START);
    context_menu::popup(popup, |ui| {
        context_menu::section(ui, &s::file_limit(easyplay_core::chat::MAX_SEND >> 30));
        if context_menu::icon_item(ui, icons::IMAGE, s::PICTURE(), None).clicked() {
            state.picks.push(PickPurpose::ChatPicture(chat));
        }
        if context_menu::icon_item(ui, icons::FILE, s::FILE(), None).clicked() {
            state.picks.push(PickPurpose::ChatFile(chat));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn tokens_start_after_a_space() {
        assert_eq!(
            token(&chars("so :sm"), false),
            Some((SuggestKind::Emoji, "sm".into(), 3))
        );
        assert_eq!(token(&chars(":fi"), false).map(|t| t.2), Some(0));
        // One letter is not enough; a colon inside a word is not a token.
        assert_eq!(token(&chars("so :s"), false), None);
        assert_eq!(token(&chars("at 10:30"), false), None);
        assert_eq!(token(&chars("x:smile"), false), None);
        assert_eq!(token(&chars("so :Smile"), false), None);
        assert_eq!(
            token(&chars("hey @jo"), true),
            Some((SuggestKind::Mention, "jo".into(), 4))
        );
        assert_eq!(
            token(&chars("@"), true),
            Some((SuggestKind::Mention, String::new(), 0))
        );
        assert_eq!(token(&chars("hey @jo"), false), None);
        assert_eq!(token(&chars("mail me@x"), true), None);
    }

    fn member(id: i64, name: &str, me: bool) -> GroupMember {
        GroupMember {
            id,
            name: name.into(),
            friend: None,
            me,
            owner: false,
            presence: easyplay_core::model::Presence::Offline,
            playing: None,
            blocked: false,
        }
    }

    #[test]
    fn mentions_are_members_named_once() {
        let list = [
            member(1000, "Sam", true),
            member(1002, "Jonas Weber", false),
            member(1003, "Dev", false),
            member(1004, "Kit!", false),
        ];
        assert_eq!(
            mentions_in(
                "@'Jonas Weber' and @Dev, @Dev again, mail@Dev, @Nobody, @'Kit!' @Jonas",
                &list
            ),
            vec![
                ("Jonas Weber".into(), 1002),
                ("Dev".into(), 1003),
                ("Kit!".into(), 1004)
            ]
        );
        assert!(mentions_in("@Sam", &list).is_empty());
        assert_eq!(handle("Dev"), "Dev");
        assert_eq!(handle("Jonas Weber"), "'Jonas Weber'");
        assert_eq!(handle("Kit!"), "'Kit!'");
        let names: Vec<&str> = members_for(&list, "we")
            .iter()
            .map(|x| x.name.as_str())
            .collect();
        assert_eq!(names, vec!["Jonas Weber"]);
        assert_eq!(members_for(&list, "").len(), 3);
    }

    #[test]
    fn formatting_puts_simplex_marks_around_the_selection() {
        assert_eq!(
            apply_format("hi there", 3, 8, Mark::Bold),
            ("hi *there*".into(), 10)
        );
        // Nothing selected: the cursor between the marks.
        assert_eq!(apply_format("hi ", 3, 3, Mark::Italic), ("hi __".into(), 4));
        // A mark opens only at the start or after a space.
        assert_eq!(apply_format("ab", 2, 2, Mark::Strike), ("ab ~~".into(), 4));
        assert_eq!(
            apply_format("a\nb", 2, 3, Mark::Code),
            ("a\n `b`".into(), 6)
        );
        // The selection's own spaces stay outside the marks.
        assert_eq!(
            apply_format("say  it  now", 3, 9, Mark::Secret),
            ("say  #it#  now".into(), 9)
        );
        assert_eq!(
            apply_format("é x", 2, 3, Mark::Color(3)),
            ("é !3 x!".into(), 7)
        );
        assert_eq!(apply_format("", 0, 0, Mark::Color(1)), ("!1 !".into(), 3));
    }

    #[test]
    fn live_formatting_keeps_every_byte_and_quiets_the_marks() {
        use super::super::markdown::Kind;
        let text = "hi *bold* _it_\n`c` #s# !1 red! @'Jonas Weber' x*";
        let r = live_ranges(text, &["Jonas Weber".to_owned()]);
        // The ranges cover the text, in order, once.
        let mut at = 0;
        for (range, _) in &r {
            assert_eq!(range.start, at, "{r:?}");
            at = range.end;
        }
        assert_eq!(at, text.len());
        let look = |piece: &str| {
            r.iter()
                .find(|(range, _)| &text[range.clone()] == piece)
                .map(|(_, l)| l.clone())
        };
        assert_eq!(look("bold"), Some(Live::Kind(Kind::Bold)));
        assert_eq!(look("it"), Some(Live::Kind(Kind::Italic)));
        assert_eq!(look("c"), Some(Live::Kind(Kind::Code)));
        assert_eq!(look("s"), Some(Live::Kind(Kind::Secret(0))));
        assert_eq!(
            look("red"),
            Some(Live::Kind(Kind::Color(super::super::markdown::Hue::Red)))
        );
        assert_eq!(look("@'Jonas Weber'"), Some(Live::Kind(Kind::Mention)));
        assert_eq!(look("!1 "), Some(Live::Mark));
        assert!(r.iter().filter(|(_, l)| *l == Live::Mark).count() >= 10);
        // Plain text stays plain.
        assert!(
            live_ranges("just words", &[])
                .iter()
                .all(|(_, l)| *l == Live::Plain)
        );
        assert!(live_ranges("", &[]).is_empty());
    }

    #[test]
    fn the_wave_and_the_clock() {
        assert!((0..500).map(wave_bar).all(|v| (0.15..=1.0).contains(&v)));
        assert_eq!(mmss(7), "0:07");
        assert_eq!(mmss(300), "5:00");
    }
}
