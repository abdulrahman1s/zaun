//! A conversation (chat.html section 8 CONVERSATION, script section 23, styles "The
//! conversation"): a 56 header (one button that opens the details, then Live · call · share ·
//! details · more), the trays under it, the messages as rows (not bubbles) and the composer.
//!
//! Rows: a person's first message has their avatar, first name and time; their following
//! messages within six minutes have neither and show the time in the gutter while pointed
//! at. Under the pointer (or with focus) a raised capsule over the row's top right offers the
//! three most used emoji, add reaction, reply and more. The messages are one tab stop:
//! ↑ ↓ Home End move, Enter opens the message's menu, R replies, E edits, Delete deletes.
//!
//! Everything a feature brings is drawn only while `ChatSnapshot::features` has it. The
//! layout mirrors in a right-to-left language: positions are worked out left to right and
//! flipped; message text lays out in its own direction (`markdown`).

use std::collections::HashMap;

use easyplay_core::model::{
    CallKind, ChatMessage, ChatRef, ChatSnapshot, Delivery, Friend, GroupMember, Membership,
    MessageAuthor, MessageBody, Presence, Reaction, StreamRecord,
};
use easyplay_core::service::ChatCommand;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Id, Key, Layout, Margin, Popup,
    PopupCloseBehavior, Pos2, Rect, RectAlign, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder,
    pos2, vec2,
};

use super::avatar::{self, Shape as AvShape};
use super::composer::{self, Compose};
use super::{Env, emoji, markdown};
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::strings::chat as c;
use crate::strings::chat::conversation as s;
use crate::theme::motion::{self, ease_out, ease_pop};
use crate::theme::palette::{self, mix};
use crate::theme::tokens::icon as icon_size;
use crate::theme::{self, Dur, Icon, Type, icons};
use crate::vm::chat::{ChatDialog, Deleting, Picker, VoicePlay};
use crate::vm::dialogs::Dialog;
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::context_menu;
use crate::widgets::tooltip;

/// The measures of the CSS (lengths are points).
mod m {
    /// `.cv-h`: 56 high, padded 16 · 18 (10 · 8 narrow), 2 between controls.
    pub const HEADER_H: f32 = 56.0;
    pub const HEAD_PAD: [f32; 2] = [16.0, 18.0];
    pub const HEAD_PAD_NARROW: [f32; 2] = [10.0, 8.0];
    pub const HEAD_GAP: f32 = 2.0;
    /// `.cv-who`: 46 high, padded 8 · 12, radius 10, 12 between avatar and words.
    pub const WHO_H: f32 = 46.0;
    pub const WHO_PAD: [f32; 2] = [8.0, 12.0];
    pub const WHO_R: u8 = 10;
    pub const WHO_GAP: f32 = 12.0;
    /// Avatars of the header and the messages.
    pub const AVATAR: f32 = 36.0;
    /// `.b.ib`: 30 square, radius 8, a 16 glyph.
    pub const BTN: f32 = 30.0;
    pub const BTN_R: u8 = 8;
    /// `.watch`: 30 high, radius 15, padded 10 · 12, 8 between, 6 before the buttons.
    pub const WATCH_PAD: [f32; 2] = [10.0, 12.0];
    pub const WATCH_GAP: f32 = 8.0;
    pub const WATCH_AFTER: f32 = 6.0;
    /// `.lv`: a 6 dot, 5 before "Live".
    pub const LIVE_DOT: f32 = 6.0;
    pub const LIVE_GAP: f32 = 5.0;
    /// `.msgs`: padded 4 · 24 · 14 (4 · 16 · 12 narrow).
    pub const MSGS_PAD: [f32; 3] = [24.0, 4.0, 14.0];
    pub const MSGS_PAD_NARROW: [f32; 3] = [16.0, 4.0, 12.0];
    /// `.cmp-w`: 0 · 24 · 18 (0 · 12 · 12 narrow).
    pub const CMP_PAD: [f32; 2] = [24.0, 18.0];
    pub const CMP_PAD_NARROW: [f32; 2] = [12.0, 12.0];
    /// The text column starts this far in (avatar 36 + 12).
    pub const INDENT: f32 = 48.0;
    /// `.ms-start`: 26 above, 6 below, at most 520 wide, lines 19; the name in display 26
    /// with 4 under it.
    pub const START_TOP: f32 = 26.0;
    pub const START_BOTTOM: f32 = 6.0;
    pub const START_MAX: f32 = 520.0;
    pub const START_LINE: f32 = 19.0;
    pub const START_GAP: f32 = 4.0;
    /// `.day`: 22 above, 6 below, 12 before the hairline.
    pub const DAY_TOP: f32 = 22.0;
    pub const DAY_BOTTOM: f32 = 6.0;
    pub const DAY_GAP: f32 = 12.0;
    /// `.m`: the plate reaches 8 past the column; padded 2 (5 on top of a first message,
    /// which has 12 above); radius 8.
    pub const OVERHANG: f32 = 8.0;
    pub const ROW_PAD: f32 = 2.0;
    pub const HEAD_TOP: f32 = 12.0;
    pub const HEAD_PAD_TOP: f32 = 5.0;
    pub const ROW_R: u8 = 8;
    /// `.m-n`: lines 18, 8 between name and time.
    pub const NAME_H: f32 = 18.0;
    pub const NAME_GAP: f32 = 8.0;
    /// `.m-g`: the gutter's time, lines 21.
    pub const GUTTER_LINE: f32 = 21.0;
    pub const GUTTER_SIZE: f32 = 10.0;
    /// `.rp`: lines 17, 1 under, 6 between, a 12 glyph.
    pub const RP_H: f32 = 17.0;
    pub const RP_BELOW: f32 = 1.0;
    pub const RP_GAP: f32 = 6.0;
    pub const RP_ICON: f32 = 12.0;
    /// `.rx`: pills 24 high, radius 12, padded 6 · 8, 5 inside, 4 apart; 5 above, 3 under;
    /// the emoji at 15.
    pub const RX_H: f32 = 24.0;
    pub const RX_PAD: [f32; 2] = [6.0, 8.0];
    pub const RX_IN: f32 = 5.0;
    pub const RX_GAP: f32 = 4.0;
    pub const RX_TOP: f32 = 5.0;
    pub const RX_BOTTOM: f32 = 3.0;
    pub const RX_EMOJI: f32 = 15.0;
    /// `.mt`: 30 high, padded 2, radius 9, 8 in from the right, 16 above the row; buttons
    /// 26 (radius 7), emoji 17, a 1 × 14 rule with 3 each side.
    pub const TOOLS_H: f32 = 30.0;
    pub const TOOLS_PAD: f32 = 2.0;
    pub const TOOLS_R: u8 = 9;
    pub const TOOLS_IN: f32 = 8.0;
    pub const TOOLS_UP: f32 = 16.0;
    pub const TOOL: f32 = 26.0;
    pub const TOOL_R: u8 = 7;
    pub const TOOL_EMOJI: f32 = 17.0;
    pub const TOOL_RULE: f32 = 14.0;
    pub const TOOL_RULE_M: f32 = 3.0;
    /// `.m-note`: 2 above, lines 16, an 11 glyph 5 from the words.
    pub const NOTE_TOP: f32 = 2.0;
    pub const NOTE_H: f32 = 16.0;
    pub const NOTE_ICON: f32 = 11.0;
    pub const NOTE_GAP: f32 = 5.0;
    /// `.vm`: 44 high, radius 22, padded 6, 10 between; play 32 with a 14 glyph; bars 2
    /// wide, 2 apart, up to 24 high (at least 3); the time 30 wide; speed 30 × 24; 3 above
    /// and under.
    pub const VM_H: f32 = 44.0;
    pub const VM_PAD: f32 = 6.0;
    pub const VM_GAP: f32 = 10.0;
    pub const VM_PLAY: f32 = 32.0;
    pub const VM_ICON: f32 = 14.0;
    pub const BAR_W: f32 = 2.0;
    pub const BAR_GAP: f32 = 2.0;
    pub const WAVE_H: f32 = 24.0;
    pub const BAR_MIN: f32 = 3.0;
    pub const VM_TIME_W: f32 = 30.0;
    pub const VM_SPEED: [f32; 2] = [30.0, 24.0];
    pub const VM_MARGIN: f32 = 3.0;
    /// Bars per second of a voice message, and their bounds.
    pub const BARS_PER_S: f32 = 2.6;
    pub const BARS: [usize; 2] = [20, 56];
    /// `.gif`: at most 280 wide, 16:10, radius 10, 4 above and under; the tag 6 in, padded
    /// 5, 16 high, radius 4; the star 26 round, 6 in, a 13 glyph.
    pub const GIF_W: f32 = 280.0;
    pub const GIF_MARGIN: f32 = 4.0;
    pub const GIF_IN: f32 = 6.0;
    pub const GIF_TAG_PAD: f32 = 5.0;
    pub const GIF_TAG_H: f32 = 16.0;
    pub const GIF_TAG_R: u8 = 4;
    pub const GIF_STAR: f32 = 26.0;
    pub const GIF_STAR_ICON: f32 = 13.0;
    /// The stand-in's word: 21% of the width, 6% in from each side.
    /// Black at 62% behind the tag and the star.
    pub const GIF_SCRIM: u8 = 158;
    /// `.sys`: at least 30 high.
    pub const SYS_H: f32 = 30.0;
    /// `.sc`: at most 440 wide, padded 8 · 10 · 8 · 8, radius 12, 12 between, a 40 cover
    /// (2:3), 3 above and under.
    pub const SC_W: f32 = 440.0;
    pub const SC_PAD: [f32; 3] = [8.0, 10.0, 8.0];
    pub const SC_R: u8 = 12;
    pub const SC_GAP: f32 = 12.0;
    pub const SC_COVER: f32 = 40.0;
    pub const SC_MARGIN: f32 = 3.0;
    /// `.cmp-off`: at least 44 high, padded 14 before and 6 after, 12 between.
    pub const OFF_H: f32 = 44.0;
    pub const OFF_PAD: [f32; 2] = [14.0, 6.0];
    pub const OFF_GAP: f32 = 12.0;
    /// A following message within this long of the one before shows no name.
    pub const RUN_MS: i64 = 6 * 60_000;
    /// Older messages load when the transcript is scrolled this close to its top.
    pub const OLDER_AT: f32 = 32.0;
    /// A new message rises this far.
    pub const RISE: f32 = 8.0;
    /// The reaction you add pops from this scale.
    pub const POP_FROM: f32 = 0.7;
    /// The longest a frame's step of a playing voice message may be.
    pub const MAX_STEP: f32 = 0.1;
}

/// Show the open conversation `chat`; `narrow`: it stands in place of the list.
pub fn show(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    narrow: bool,
) {
    let name = super::name_of(snap, chat);
    let preview = snap.chats.iter().find(|p| p.chat == chat);
    note_unread_at_open(ui, state, chat, preview.map_or(0, |p| p.unread));
    // Read while it is open and the window has focus: once per latest message.
    if let Some(pv) = preview
        && super::window_focused(ui)
        && state
            .chat
            .should_mark(chat, pv.last.as_ref().map(|m| m.id), pv.unread)
    {
        env.bridge
            .send(UiCommand::Chat(ChatCommand::MarkRead(chat)));
    }
    settle_deletes(state, env);
    tick_voice(ui, state);
    let invited = match chat {
        ChatRef::Group(g) => snap
            .group(g)
            .is_some_and(|g| g.membership == Membership::Invited),
        ChatRef::Direct(_) => false,
    };
    let compose = match chat {
        ChatRef::Direct(f) if !snap.friend(f).is_some_and(|f| f.ready) => {
            Compose::Closed(c::waiting(&name))
        }
        ChatRef::Group(g) => match snap.group(g).map(|g| g.membership) {
            Some(Membership::Joining) => Compose::Closed(c::JOINING().to_owned()),
            Some(Membership::Left) | None => Compose::Closed(c::LEFT().to_owned()),
            _ => Compose::Open,
        },
        ChatRef::Direct(_) => Compose::Open,
    };
    let pad = if narrow {
        m::HEAD_PAD_NARROW
    } else {
        m::HEAD_PAD
    };
    egui::Panel::top("chat.head")
        .exact_size(m::HEADER_H)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.inner_margin(Margin {
            left: pad[0] as i8,
            right: pad[1] as i8,
            top: 0,
            bottom: 0,
        }))
        .show(ui, |ui| header(ui, state, env, snap, chat, &name, narrow));
    let cp = if narrow {
        m::CMP_PAD_NARROW
    } else {
        m::CMP_PAD
    };
    egui::Panel::bottom("chat.compose")
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.inner_margin(Margin {
            left: cp[0] as i8,
            right: cp[0] as i8,
            top: 0,
            bottom: cp[1] as i8,
        }))
        .show(ui, |ui| {
            if invited {
                invitation(ui, env, snap, chat, &name);
            } else {
                composer::show(ui, state, env, chat, &name, &compose);
            }
        });
    egui::CentralPanel::no_frame().show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        super::trays::show(ui, state, env, snap, chat);
        transcript(ui, state, env, snap, chat, &name, narrow);
    });
    viewer(ui, state, env, chat);
}

/// The picture viewer over the whole window, while one of this chat's pictures is open.
fn viewer(ui: &Ui, state: &mut UiState, env: &mut Env<'_>, chat: ChatRef) {
    let Some(v) = state.chat.viewer.clone() else {
        return;
    };
    let media = state.chat.open.as_ref().and_then(|o| {
        o.messages
            .iter()
            .find(|m| m.id == v.item)
            .and_then(|m| match &m.body {
                MessageBody::Image(md) => Some(md.clone()),
                MessageBody::Gif(n) => still_picture(n),
                _ => None,
            })
    });
    let Some(media) = media.filter(|_| v.chat == chat) else {
        state.chat.viewer = None;
        return;
    };
    let key = format!("{chat:?}:{}", v.item);
    match super::media::viewer(ui.ctx(), &key, &media) {
        Some(super::media::ViewerAct::Close) => state.chat.viewer = None,
        Some(super::media::ViewerAct::Open) => {
            env.bridge.send(UiCommand::Chat(ChatCommand::OpenFile {
                chat,
                item: v.item,
            }));
        }
        None => {}
    }
}

// ---- Direction ---------------------------------------------------------------------------

/// Mirrors rectangles inside `outer` when the interface reads right to left.
#[derive(Clone, Copy)]
struct Flip {
    outer: Rect,
    rtl: bool,
}

impl Flip {
    fn new(outer: Rect) -> Self {
        Self {
            outer,
            rtl: crate::i18n::dir() == crate::i18n::Dir::Rtl,
        }
    }

    fn r(self, r: Rect) -> Rect {
        if !self.rtl {
            return r;
        }
        let l = self.outer.left() + self.outer.right() - r.right();
        Rect::from_min_size(pos2(l, r.top()), r.size())
    }

    fn p(self, p: Pos2) -> Pos2 {
        if self.rtl {
            pos2(self.outer.left() + self.outer.right() - p.x, p.y)
        } else {
            p
        }
    }
}

// ---- People --------------------------------------------------------------------------------

/// Who wrote a message, as the transcript shows them.
#[derive(Clone)]
struct Who {
    /// Full name (the avatar's initials).
    name: String,
    /// "You", or the first name.
    short: String,
    paper: Option<u8>,
    mine: bool,
    /// A group member the user blocked.
    blocked: bool,
    /// The member's number (groups).
    member: Option<i64>,
    /// The friend behind them.
    friend: Option<i64>,
}

fn first(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_owned()
}

fn who(snap: &ChatSnapshot, chat: ChatRef, author: &MessageAuthor) -> Who {
    match author {
        MessageAuthor::Me => {
            let (name, paper) = snap
                .profile
                .as_ref()
                .map_or((String::new(), None), |p| (p.name.clone(), p.paper));
            Who {
                name,
                short: s::YOU().to_owned(),
                paper,
                mine: true,
                blocked: false,
                member: None,
                friend: None,
            }
        }
        MessageAuthor::Friend => {
            let f = match chat {
                ChatRef::Direct(f) => snap.friend(f),
                ChatRef::Group(_) => None,
            };
            let name = f.map(|f| f.name.clone()).unwrap_or_default();
            Who {
                short: first(&name),
                name,
                paper: f.and_then(|f| f.paper),
                mine: false,
                blocked: false,
                member: None,
                friend: f.map(|f| f.id),
            }
        }
        MessageAuthor::Member(n) => {
            let mem = member(snap, chat, n);
            let friend = mem.and_then(|m| m.friend).and_then(|f| snap.friend(f));
            Who {
                name: n.clone(),
                short: first(n),
                paper: friend.and_then(|f| f.paper),
                mine: false,
                blocked: mem.is_some_and(|m| m.blocked),
                member: mem.map(|m| m.id),
                friend: mem.and_then(|m| m.friend),
            }
        }
    }
}

fn member<'a>(snap: &'a ChatSnapshot, chat: ChatRef, name: &str) -> Option<&'a GroupMember> {
    match chat {
        ChatRef::Group(g) => snap
            .group(g)
            .and_then(|g| g.member_list.iter().find(|m| m.name == name && !m.me)),
        ChatRef::Direct(_) => None,
    }
}

// ---- Header --------------------------------------------------------------------------------

/// The status words under the header's name, and whether they are accent ("Playing …").
fn status_line(env: &Env<'_>, snap: &ChatSnapshot, chat: ChatRef) -> (String, bool) {
    let presence = snap.features.presence;
    match chat {
        ChatRef::Direct(f) => {
            let Some(f) = snap.friend(f) else {
                return (String::new(), false);
            };
            if f.blocked {
                return (s::BLOCKED().to_owned(), false);
            }
            if !f.ready {
                return (s::CONNECTING().to_owned(), false);
            }
            if !presence {
                return (String::new(), false);
            }
            friend_status(env, f)
        }
        ChatRef::Group(g) => {
            let Some(g) = snap.group(g) else {
                return (String::new(), false);
            };
            let n = if g.member_list.is_empty() {
                g.members.unwrap_or(0)
            } else {
                u32::try_from(g.member_list.len()).unwrap_or(u32::MAX)
            };
            if !presence || g.member_list.is_empty() {
                return (c::members(n), false);
            }
            let online = g
                .member_list
                .iter()
                .filter(|m| m.me || m.presence != Presence::Offline)
                .count();
            (
                s::group_status(n, u32::try_from(online).unwrap_or(u32::MAX)),
                false,
            )
        }
    }
}

fn friend_status(env: &Env<'_>, f: &Friend) -> (String, bool) {
    if let Some(pl) = &f.playing
        && f.presence != Presence::Offline
    {
        let t = crate::strings::fmt::play_time(u64::from(pl.minutes) * 60_000);
        return (s::playing(&pl.title, &t), true);
    }
    let mins = f
        .last_seen
        .map(|t| ((env.now.as_millis() - t.as_millis()).max(0) / 60_000) as u64);
    let words = match f.presence {
        Presence::Online => s::ONLINE().to_owned(),
        Presence::Busy => s::BUSY().to_owned(),
        Presence::Away => match mins {
            Some(n) if n > 0 => s::away_for(&crate::strings::fmt::play_time(n * 60_000)),
            _ => s::AWAY().to_owned(),
        },
        Presence::Offline => match mins {
            None => s::OFFLINE().to_owned(),
            Some(0) => s::SEEN_NOW().to_owned(),
            Some(n) if n < 60 => s::seen_minutes(n),
            Some(n) if n < 24 * 60 => s::seen_hours(n / 60),
            Some(n) if n < 48 * 60 => s::SEEN_YESTERDAY().to_owned(),
            Some(n) => s::seen_days(n / (24 * 60)),
        },
    };
    (words, false)
}

/// Someone in this conversation shares a game right now (not the user): their name.
fn live_sharer(snap: &ChatSnapshot, chat: ChatRef) -> Option<(String, String)> {
    match chat {
        ChatRef::Direct(f) => snap.friend(f).and_then(|f| {
            f.playing
                .as_ref()
                .filter(|p| p.live && !f.blocked)
                .map(|p| (f.name.clone(), p.title.clone()))
        }),
        ChatRef::Group(g) => snap.group(g).and_then(|g| {
            g.member_list.iter().find_map(|m| {
                m.playing
                    .as_ref()
                    .filter(|p| p.live && !m.blocked && !m.me)
                    .map(|p| (m.name.clone(), p.title.clone()))
            })
        }),
    }
}

/// The details panel is open (`None` follows the width: open from 1240).
fn details_open(ui: &Ui, state: &UiState) -> bool {
    state
        .chat
        .details
        .unwrap_or(ui.ctx().content_rect().width() >= 1240.0)
}

/// The header: back (narrow), the details button, then Live · call · share · details · more.
fn header(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    name: &str,
    narrow: bool,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let area = ui.max_rect();
    let flip = Flip::new(area);
    let cy = area.center().y;
    let f = &snap.features;
    let friend = match chat {
        ChatRef::Direct(id) => snap.friend(id),
        ChatRef::Group(_) => None,
    };
    let blocked = friend.is_some_and(|f| f.blocked);
    let offline = friend.is_some_and(|fr| f.presence && fr.presence == Presence::Offline);
    let in_call = snap.live.call.as_ref().is_some_and(|c| c.chat == chat);
    let sharing = snap.live.share.as_ref().is_some_and(|s| s.chat == chat);
    let watching = snap.live.watch.as_ref().is_some_and(|w| w.chat == chat);
    let group = matches!(chat, ChatRef::Group(_));
    let what = if group {
        s::GROUP_DETAILS()
    } else {
        s::PROFILE()
    };
    // From the right: more, details, share, call, Live.
    let mut right = area.right();
    let mut slot = |w: f32| {
        let r = Rect::from_min_size(pos2(right - w, cy - m::BTN / 2.0), vec2(w, m::BTN));
        right -= w + m::HEAD_GAP;
        r
    };
    let more_r = slot(m::BTN);
    let details_r = (!narrow).then(|| slot(m::BTN));
    let share_r = (f.share && !blocked).then(|| slot(m::BTN));
    let call_r = (f.calls && !blocked).then(|| slot(m::BTN));
    // More.
    let more = head_button(
        ui,
        flip.r(more_r),
        "chat.head.more",
        icons::DOTS,
        s::MORE(),
        HeadLook::default(),
    );
    header_menu(state, env, snap, chat, &more);
    if let Some(r) = details_r {
        let open = details_open(ui, state);
        let resp = head_button(
            ui,
            flip.r(r),
            "chat.head.details",
            icons::SIDEBAR,
            what,
            HeadLook {
                pressed: Some(open),
                ..HeadLook::default()
            },
        );
        if resp.clicked() {
            state.chat.details = Some(!open);
        }
    }
    if let Some(r) = share_r {
        let tip = if sharing { s::STOP_SHARE() } else { s::SHARE() };
        let off = friend
            .filter(|_| offline)
            .map(|fr| s::offline(&first(&fr.name)));
        let resp = head_button(
            ui,
            flip.r(r),
            "chat.head.share",
            icons::SHARE_GAME,
            tip,
            HeadLook {
                pressed: Some(sharing),
                accent: sharing,
                disabled: off.clone(),
            },
        );
        if resp.clicked() {
            share_flow(state, env, snap, chat);
        }
    }
    if let Some(r) = call_r {
        let tip = if in_call {
            s::IN_CALL()
        } else if !group {
            s::CALL()
        } else if snap
            .group(match chat {
                ChatRef::Group(g) => g,
                ChatRef::Direct(_) => -1,
            })
            .is_some_and(|g| !g.voice.is_empty())
        {
            s::JOIN_VOICE()
        } else {
            s::START_VOICE()
        };
        let disabled = if in_call {
            Some(s::IN_CALL().to_owned())
        } else {
            friend
                .filter(|_| offline)
                .map(|fr| s::offline(&first(&fr.name)))
        };
        let resp = head_button(
            ui,
            flip.r(r),
            "chat.head.call",
            icons::PHONE,
            tip,
            HeadLook {
                disabled,
                ..HeadLook::default()
            },
        );
        if resp.clicked() {
            call_flow(state, env, snap, chat);
        }
    }
    // Live: watch whoever shares here.
    if f.share
        && !watching
        && let Some((who_name, game)) = live_sharer(snap, chat)
    {
        let fname = first(&who_name);
        let label = s::watch(&fname);
        let live_g = Type::MicroStrong.galley(ui, s::LIVE(), p.coral, f32::INFINITY, 1);
        let words = (!narrow).then(|| Type::BodyStrong.galley(ui, &label, p.t1, 200.0, 1));
        let w = m::WATCH_PAD[0]
            + m::LIVE_DOT
            + m::LIVE_GAP
            + live_g.size().x
            + words.as_ref().map_or(0.0, |g| m::WATCH_GAP + g.size().x)
            + m::WATCH_PAD[1];
        right -= m::WATCH_AFTER - m::HEAD_GAP;
        let r = Rect::from_min_size(pos2(right - w, cy - m::BTN / 2.0), vec2(w, m::BTN));
        right -= w + m::HEAD_GAP;
        let fr = flip.r(r);
        let resp = ui.interact(fr, ui.make_persistent_id("chat.head.watch"), Sense::click());
        crate::a11y::button(&resp, &label);
        let hover = crate::widgets::hover_t(&resp, &theme);
        let corner = CornerRadius::same(theme::tokens::radius::PILL);
        ui.painter().rect_filled(fr, corner, mix(p.o1, p.o2, hover));
        let mut x = r.left() + m::WATCH_PAD[0];
        ui.painter().circle_filled(
            flip.p(pos2(x + m::LIVE_DOT / 2.0, cy)),
            m::LIVE_DOT / 2.0,
            p.coral,
        );
        x += m::LIVE_DOT + m::LIVE_GAP;
        let lw = live_g.size().x;
        let lr = flip.r(Rect::from_min_size(
            pos2(x, cy - live_g.size().y / 2.0),
            live_g.size(),
        ));
        ui.painter().galley(lr.min, live_g, p.coral);
        x += lw + m::WATCH_GAP;
        if let Some(g) = words {
            let wr = flip.r(Rect::from_min_size(
                pos2(x, cy - g.size().y / 2.0),
                g.size(),
            ));
            ui.painter().galley(wr.min, g, p.t1);
        }
        if resp.has_focus() {
            crate::widgets::focus_ring(ui.painter(), fr, corner, &theme);
        }
        tooltip::attach(resp.clone(), &s::watch_tip(&fname, &game));
        if resp.clicked() {
            env.bridge.send(UiCommand::Chat(ChatCommand::Watch(chat)));
        }
    }
    // Left: back on a narrow window, then the details button.
    let mut left = area.left();
    if narrow {
        let r = Rect::from_min_size(pos2(left, cy - m::BTN / 2.0), vec2(m::BTN, m::BTN));
        let back = head_button(
            ui,
            flip.r(r),
            "chat.head.back",
            if flip.rtl {
                icons::CARET_RIGHT
            } else {
                icons::CARET_LEFT
            },
            s::BACK(),
            HeadLook::default(),
        );
        if back.clicked() {
            state.chat.close_chat();
            return;
        }
        left += m::BTN + m::HEAD_GAP;
    }
    let (status, accent) = status_line(env, snap, chat);
    let max_w = (right - m::HEAD_GAP - left).max(m::AVATAR + m::WHO_PAD[0]);
    let text_max = (max_w - m::WHO_PAD[0] - m::AVATAR - m::WHO_GAP - m::WHO_PAD[1]).max(1.0);
    let ng = Type::Heading.galley(ui, name, p.t1, text_max, 1);
    let sc = if accent { p.accent_text } else { p.t3 };
    let sg = (!status.is_empty()).then(|| Type::Caption.galley(ui, &status, sc, text_max, 1));
    let tw = ng.size().x.max(sg.as_ref().map_or(0.0, |g| g.size().x));
    let w = (m::WHO_PAD[0] + m::AVATAR + m::WHO_GAP + tw + m::WHO_PAD[1]).min(max_w);
    let who_r = Rect::from_min_size(pos2(left, cy - m::WHO_H / 2.0), vec2(w, m::WHO_H));
    let fr = flip.r(who_r);
    let resp = ui.interact(fr, ui.make_persistent_id("chat.head.who"), Sense::click());
    let spoken = if status.is_empty() {
        name.to_owned()
    } else {
        c::header_spoken(name, &status)
    };
    crate::a11y::button(&resp, &spoken);
    let hover = crate::widgets::hover_t(&resp, &theme);
    let corner = CornerRadius::same(m::WHO_R);
    let fill = palette::fade(p.o1, hover);
    ui.painter().rect_filled(fr, corner, fill);
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), fr, corner, &theme);
    }
    let ar = flip.r(Rect::from_min_size(
        pos2(left + m::WHO_PAD[0], cy - m::AVATAR / 2.0),
        vec2(m::AVATAR, m::AVATAR),
    ));
    let (av_name, paper, shape) = match chat {
        ChatRef::Direct(_) => (name, friend.and_then(|f| f.paper), AvShape::Person),
        ChatRef::Group(_) => (name, None, AvShape::Group),
    };
    avatar::avatar(ui.painter(), ar, av_name, paper, shape);
    if let Some(fr) = friend
        && f.presence
        && !fr.blocked
    {
        avatar::status(ui.painter(), ar, fr.presence, mix(p.bg, p.o1, hover), false);
    }
    let tx = left + m::WHO_PAD[0] + m::AVATAR + m::WHO_GAP;
    let th = ng.size().y + sg.as_ref().map_or(0.0, |g| g.size().y);
    let ty = cy - th / 2.0;
    let nh = ng.size().y;
    let nr = flip.r(Rect::from_min_size(pos2(tx, ty), ng.size()));
    ui.painter().galley(nr.min, ng, p.t1);
    if let Some(g) = sg {
        let r = flip.r(Rect::from_min_size(pos2(tx, ty + nh), g.size()));
        ui.painter().galley(r.min, g, sc);
    }
    tooltip::attach(resp.clone(), what);
    if resp.clicked() {
        let open = details_open(ui, state);
        state.chat.details = Some(!open);
    }
}

/// How a header button looks.
#[derive(Default)]
struct HeadLook {
    /// A toggle, pressed or not (`o2` and primary glyph while pressed).
    pressed: Option<bool>,
    /// Accent glyph (sharing here).
    accent: bool,
    /// Greyed, with this reason as the tooltip.
    disabled: Option<String>,
}

/// A `.b.ib`: 30 square, radius 8, a 16 glyph; `o2` under the pointer.
fn head_button(
    ui: &mut Ui,
    rect: Rect,
    id: &str,
    icon: Icon,
    label: &str,
    look: HeadLook,
) -> egui::Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let enabled = look.disabled.is_none();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, ui.make_persistent_id(id), sense);
    match look.pressed {
        Some(on) => crate::a11y::toggle(&resp, label, on),
        None => crate::a11y::button(&resp, label),
    }
    let hover = if enabled {
        crate::widgets::hover_t(&resp, &theme)
    } else {
        0.0
    };
    let pressed = look.pressed == Some(true);
    let fill = if pressed {
        p.o2
    } else {
        palette::fade(p.o2, hover)
    };
    let corner = CornerRadius::same(m::BTN_R);
    ui.painter().rect_filled(rect, corner, fill);
    let color = if !enabled {
        p.t4
    } else if look.accent {
        p.accent_text
    } else if pressed {
        p.t1
    } else {
        mix(p.t2, p.t1, hover)
    };
    paint_icon(ui.painter(), rect.center(), icon, icon_size::INLINE, color);
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), rect, corner, &theme);
    }
    match &look.disabled {
        Some(why) => tooltip::disabled(resp, why),
        None => tooltip::attach(resp, label),
    }
}

fn paint_icon(painter: &egui::Painter, at: Pos2, icon: Icon, size: f32, color: Color32) {
    painter.text(
        at,
        Align2::CENTER_CENTER,
        icon.text(false),
        icons::font(size),
        color,
    );
}

/// The conversation's menu (with icons): Mute ✓ · Check the connection… · Invite people… ·
/// — · Block / Unblock · Remove friend… or Leave group….
fn header_menu(
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    resp: &egui::Response,
) {
    let f = snap.features;
    let muted = snap
        .chats
        .iter()
        .find(|p| p.chat == chat)
        .is_some_and(|p| p.muted);
    let popup = Popup::menu(resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END);
    let mut dialog = None;
    let mut cmd = None;
    context_menu::popup(popup, |ui| match chat {
        ChatRef::Direct(id) => {
            let friend = snap.friend(id);
            let fname = friend.map(|f| first(&f.name)).unwrap_or_default();
            if f.mute
                && context_menu::icon_check_item(ui, icons::BELL_OFF, s::MUTE(), muted, false)
                    .clicked()
            {
                cmd = Some(ChatCommand::SetMuted { chat, on: !muted });
            }
            if f.verify
                && friend.is_some_and(|f| !f.blocked)
                && context_menu::icon_item(ui, icons::SHIELD_CHECK, s::CHECK(), None).clicked()
            {
                dialog = Some(ChatDialog::Verify(id));
                ui.close();
            }
            if f.mute || f.verify {
                context_menu::separator(ui);
            }
            if f.block {
                if friend.is_some_and(|f| f.blocked) {
                    if context_menu::icon_item(ui, icons::PROHIBIT, &s::unblock(&fname), None)
                        .clicked()
                    {
                        cmd = Some(ChatCommand::SetBlocked {
                            friend: id,
                            on: false,
                        });
                        ui.close();
                    }
                } else if context_menu::icon_danger_item(
                    ui,
                    icons::PROHIBIT,
                    &s::block(&fname),
                    None,
                )
                .clicked()
                {
                    dialog = Some(ChatDialog::Block(id));
                    ui.close();
                }
            }
            if context_menu::icon_danger_item(ui, icons::USER_MINUS, s::REMOVE_FRIEND(), None)
                .clicked()
            {
                dialog = Some(ChatDialog::RemoveFriend(id));
                ui.close();
            }
        }
        ChatRef::Group(g) => {
            if f.mute
                && context_menu::icon_check_item(ui, icons::BELL_OFF, s::MUTE(), muted, false)
                    .clicked()
            {
                cmd = Some(ChatCommand::SetMuted { chat, on: !muted });
            }
            if context_menu::icon_item(ui, icons::USER_PLUS, s::INVITE(), None).clicked() {
                dialog = Some(ChatDialog::InviteToGroup(g));
                ui.close();
            }
            context_menu::separator(ui);
            if context_menu::icon_danger_item(ui, icons::LEAVE, s::LEAVE_GROUP(), None).clicked() {
                dialog = Some(ChatDialog::LeaveGroup(g));
                ui.close();
            }
        }
    });
    if let Some(cmd) = cmd {
        env.bridge.send(UiCommand::Chat(cmd));
    }
    if let Some(d) = dialog {
        state.show_dialog(Dialog::Chat(d));
    }
}

/// Call a friend, or start or join a group's voice.
fn call_flow(state: &mut UiState, env: &mut Env<'_>, snap: &ChatSnapshot, chat: ChatRef) {
    super::dialogs::call_flow(state, env, snap, chat);
}

/// Share your game here, or stop sharing it.
fn share_flow(state: &mut UiState, env: &mut Env<'_>, snap: &ChatSnapshot, chat: ChatRef) {
    super::dialogs::share_flow(state, env, snap, chat);
}

/// A group the user is invited to, where the composer goes (`.cmp-off`): "Jonas invited you
/// to LAN night" with Join (filled) and Decline (boxed).
fn invitation(ui: &mut Ui, env: &mut Env<'_>, snap: &ChatSnapshot, chat: ChatRef, name: &str) {
    let ChatRef::Group(g) = chat else {
        return;
    };
    let p = theme::theme(ui.ctx()).palette.clone();
    let owner = snap
        .group(g)
        .and_then(|g| g.member_list.iter().find(|m| m.owner && !m.me))
        .map(|m| first(&m.name));
    let text = match &owner {
        Some(who) => s::invited_by(who, name),
        None => c::invited_title(name),
    };
    let join = Button::primary(c::JOIN());
    let decline = Button::new(c::DECLINE());
    let (jw, dw) = (join.width(ui), decline.width(ui));
    let w = ui.available_width();
    let text_w = (w - m::OFF_PAD[0] - m::OFF_PAD[1] - jw - dw - m::OFF_GAP * 3.0).max(1.0);
    let para = super::paint::Para::new(ui, &text, Type::Body, p.t2, text_w);
    let h = (para.size().y + m::OFF_PAD[1] * 2.0).max(m::OFF_H);
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    crate::a11y::item(&resp, egui::Role::Label, &text);
    let corner = CornerRadius::same(theme::tokens::radius::LG);
    ui.painter().rect_filled(r, corner, p.o1);
    if p.outlined() {
        ui.painter()
            .rect_stroke(r, corner, Stroke::new(1.0, p.line), StrokeKind::Inside);
    }
    let flip = Flip::new(r);
    let ps = para.size();
    let tr = flip.r(Rect::from_min_size(
        pos2(r.left() + m::OFF_PAD[0], r.center().y - ps.y / 2.0),
        vec2(text_w, ps.y),
    ));
    para.paint(ui.painter(), tr, p.t2);
    let bh = theme::tokens::control::BUTTON;
    let dr = flip.r(Rect::from_min_size(
        pos2(r.right() - m::OFF_PAD[1] - dw, r.center().y - bh / 2.0),
        vec2(dw, bh),
    ));
    let jr = flip.r(Rect::from_min_size(
        pos2(
            r.right() - m::OFF_PAD[1] - dw - m::OFF_GAP - jw,
            r.center().y - bh / 2.0,
        ),
        vec2(jw, bh),
    ));
    if ui.put(jr, join).clicked() {
        env.bridge.send(UiCommand::Chat(ChatCommand::JoinGroup(g)));
    }
    if ui.put(dr, decline).clicked() {
        env.bridge.send(UiCommand::Chat(ChatCommand::LeaveGroup(g)));
    }
}

// ---- State across frames --------------------------------------------------------------------

/// The unread count when the conversation opened (before it was marked read), so "New" can
/// stand above the first of them once the messages are in.
fn note_unread_at_open(ui: &Ui, state: &UiState, chat: ChatRef, unread: u32) {
    let key = Id::unique(("chat.unread_at_open", chat));
    let fresh = state.chat.open.as_ref().is_some_and(|o| !o.settled);
    let had = ui.ctx().data(|d| d.get_temp::<u32>(key));
    if fresh && had.is_none() {
        ui.ctx().data_mut(|d| d.insert_temp(key, unread));
    }
}

/// Deleted messages go out once their toast is gone; Undo (the toast's key used up)
/// keeps them.
fn settle_deletes(state: &mut UiState, env: &mut Env<'_>) {
    if state.chat.deleting.is_empty() {
        return;
    }
    let showing: Vec<u64> = state
        .toasts
        .visible()
        .iter()
        .filter_map(|t| t.action.as_ref().map(|a| a.key))
        .collect();
    let mut keep = Vec::new();
    for d in std::mem::take(&mut state.chat.deleting) {
        if !state.undo.contains_key(&d.undo_key) {
            continue; // Undone.
        }
        if showing.contains(&d.undo_key) {
            keep.push(d);
            continue;
        }
        state.undo.remove(&d.undo_key);
        env.bridge.send(UiCommand::Chat(ChatCommand::Delete {
            chat: d.chat,
            item: d.item,
        }));
    }
    state.chat.deleting = keep;
}

/// A playing voice message moves on (at its speed) and stops at its end.
fn tick_voice(ui: &Ui, state: &mut UiState) {
    let Some(v) = state.chat.voice.as_mut() else {
        return;
    };
    if v.paused {
        return;
    }
    let secs = state.chat.open.as_ref().and_then(|o| {
        o.messages
            .iter()
            .find(|m| m.id == v.item)
            .and_then(|m| match &m.body {
                MessageBody::Voice { secs, .. } => Some(*secs as f32),
                _ => None,
            })
    });
    let Some(secs) = secs else {
        state.chat.voice = None;
        return;
    };
    let dt = ui.input(|i| i.stable_dt).min(m::MAX_STEP);
    v.pos += dt * v.speed;
    if v.pos >= secs {
        state.chat.voice = None;
    } else {
        ui.ctx().request_repaint();
    }
}

/// When new messages arrived (by id), for their rise.
#[derive(Clone, Default)]
struct Fresh {
    max: Option<i64>,
    at: HashMap<i64, f64>,
}

/// The message the transcript's keys are on, and a request to bring it into view.
#[derive(Clone, Copy, Default)]
struct Active {
    item: Option<i64>,
    reveal: bool,
}

/// An open message menu: the message and what it hangs from.
#[derive(Clone, Copy)]
struct MenuAt {
    item: i64,
    anchor: Rect,
}

fn menu_key() -> Id {
    Id::unique("chat.msg.menu")
}

// ---- Transcript -----------------------------------------------------------------------------

fn local_day(t: easyplay_core::clock::Timestamp, offset_s: i32) -> i64 {
    (t.as_millis() + i64::from(offset_s) * 1000).div_euclid(86_400_000)
}

/// Is `m` a line that happened (a call, an event) rather than something someone wrote?
fn is_sys(m: &ChatMessage) -> bool {
    matches!(m.body, MessageBody::Call(_) | MessageBody::Notice(_))
}

/// The transcript: the start, day lines, "New", messages; older ones loading at the top.
fn transcript(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    name: &str,
    narrow: bool,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let motion = theme.motion;
    let key = Id::unique(("chat.transcript", chat));
    let Some(conv) = state.chat.open.as_ref() else {
        return;
    };
    let shift: Option<f32> = ui.ctx().data_mut(|d| d.remove_temp(key.with("shift")));
    let mut area = egui::ScrollArea::vertical()
        .id_salt(key)
        .auto_shrink([false, false])
        .stick_to_bottom(true);
    if let Some(y) = shift {
        area = area.vertical_scroll_offset(y);
    }
    let deleting: Vec<i64> = state
        .chat
        .deleting
        .iter()
        .filter(|d| d.chat == chat)
        .map(|d| d.item)
        .collect();
    let messages: Vec<ChatMessage> = conv
        .messages
        .iter()
        .filter(|m| !deleting.contains(&m.id))
        .cloned()
        .collect();
    let loading_older = conv.pending.is_some_and(|(_, older)| older);
    let loading_first = !conv.loaded && conv.failed.is_none();
    let failed = conv.failed.is_some();
    let complete = conv.complete && conv.loaded;
    let settle = conv.loaded && !conv.settled && !messages.is_empty();
    // "New" above the first unread message, once the messages are in.
    if conv.loaded && state.chat.new_from.is_none_or(|(c, _)| c != chat) {
        let at_open: u32 = ui
            .ctx()
            .data(|d| d.get_temp(Id::unique(("chat.unread_at_open", chat))))
            .unwrap_or(0);
        let first_unread = messages
            .iter()
            .find(|m| m.unread && m.author != MessageAuthor::Me)
            .map(|m| m.id)
            .or_else(|| {
                let n = at_open as usize;
                (n > 0 && n <= messages.len()).then(|| messages[messages.len() - n].id)
            });
        if let Some(id) = first_unread {
            state.chat.new_from = Some((chat, id));
        } else if conv.settled {
            state.chat.new_from = Some((chat, i64::MIN));
        }
    }
    let new_from = state
        .chat
        .new_from
        .filter(|(c, _)| *c == chat)
        .map(|(_, id)| id);
    // Messages that arrived since the transcript settled rise into place.
    let fresh_key = key.with("fresh");
    let mut fresh: Fresh = ui.ctx().data(|d| d.get_temp(fresh_key)).unwrap_or_default();
    let now = ui.input(|i| i.time);
    let newest = messages.last().map(|m| m.id);
    if conv.settled {
        if let (Some(max), Some(n)) = (fresh.max, newest) {
            for m in messages.iter().filter(|m| m.id > max) {
                fresh.at.entry(m.id).or_insert(now);
            }
            let _ = n;
        }
        fresh.max = newest.max(fresh.max);
    } else {
        fresh.max = newest;
    }
    let rise_s = motion.fade(Dur::Base);
    fresh.at.retain(|_, t| now - *t < f64::from(rise_s));
    ui.ctx()
        .data_mut(|d| d.insert_temp(fresh_key, fresh.clone()));
    if !fresh.at.is_empty() {
        ui.ctx().request_repaint();
    }
    // The keys' message: the last one until moved.
    let active_key = key.with("active");
    let mut active: Active = ui
        .ctx()
        .data(|d| d.get_temp(active_key))
        .unwrap_or_default();
    if active
        .item
        .is_none_or(|a| !messages.iter().any(|m| m.id == a))
    {
        active.item = messages.last().map(|m| m.id);
    }
    // Changing chats fades the messages in.
    let alpha = motion::appear(
        ui.ctx(),
        Id::unique(("chat.fade", chat)),
        motion.fade(Dur::Fast),
    );
    let pad = if narrow {
        m::MSGS_PAD_NARROW
    } else {
        m::MSGS_PAD
    };
    let group = matches!(chat, ChatRef::Group(_));
    let gif_autoplay = env.bridge.prefs().chat.gif_autoplay;
    let mut acts: Vec<Act> = Vec::new();
    // While the transcript scrolls (and until the pointer moves again a little after it
    // stopped), no row shows its tools, hover fill or gutter time.
    let gate_key = key.with("pointing");
    let mut gate: Pointing = ui.ctx().data(|d| d.get_temp(gate_key)).unwrap_or_default();
    let now_s = ui.input(|i| i.time);
    let over_log = ui.rect_contains_pointer(ui.available_rect_before_wrap());
    let wheel = ui.input(|i| i.is_scrolling() || i.smooth_scroll_delta != egui::Vec2::ZERO);
    if wheel && over_log {
        gate.quiet_since = Some(now_s);
    } else if let Some(t) = gate.quiet_since
        && now_s - t >= SCROLL_GRACE
        && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
    {
        gate.quiet_since = None;
    }
    let quiet = gate.quiet_since.is_some();
    // The capsule hangs over the row above: while the pointer is on the capsule shown last
    // pass, its row keeps the tools and no other row lights up.
    let pointer = ui.ctx().pointer_hover_pos();
    let lock = gate
        .owner
        .filter(|(_, r)| pointer.is_some_and(|p| r.contains(p)))
        .map(|(item, _)| item);
    let mut owner: Option<(i64, Rect)> = None;
    let out = area.show(ui, |ui| {
        ui.multiply_opacity(alpha);
        egui::Frame::NONE
            .inner_margin(Margin {
                left: pad[0] as i8,
                right: pad[0] as i8,
                top: pad[1] as i8,
                bottom: pad[2] as i8,
            })
            .show(ui, |ui| {
                let w = ui.available_width();
                let at = ui.cursor().min;
                let mut col = ui.new_child(
                    UiBuilder::new()
                        .id_salt("chat.log")
                        .max_rect(Rect::from_min_size(at, vec2(w, f32::INFINITY)))
                        .layout(Layout::top_down(Align::Min)),
                );
                col.set_width(w);
                col.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if complete || (messages.is_empty() && conv_loaded(state)) {
                    start_block(&mut col, name, group);
                }
                if loading_older {
                    super::quiet_line(&mut col, s::LOADING_OLDER(), p.t3, true);
                }
                if failed {
                    super::quiet_line(&mut col, s::LOAD_FAILED(), p.coral, true);
                }
                if loading_first {
                    skeleton_messages(&mut col);
                }
                let mut prev: Option<&ChatMessage> = None;
                let mut after_line = true;
                for msg in &messages {
                    let day = local_day(msg.at, env.utc_offset_s);
                    let new_day = prev.is_none_or(|q| local_day(q.at, env.utc_offset_s) != day);
                    if new_day {
                        let label = crate::strings::stats::day(msg.at, env.now, env.utc_offset_s);
                        day_line(&mut col, &label, false);
                        after_line = true;
                    }
                    let is_new = new_from == Some(msg.id);
                    if is_new {
                        day_line(&mut col, s::NEW(), true);
                        after_line = true;
                    }
                    let joined = !new_day && !is_new && prev.is_some_and(|q| continues(q, msg));
                    let rise = fresh
                        .at
                        .get(&msg.id)
                        .map(|t| ((now - t) as f32 / rise_s.max(0.001)).clamp(0.0, 1.0));
                    let ctx = RowCtx {
                        chat,
                        head: !joined,
                        after_line,
                        active: active.item == Some(msg.id),
                        reveal: active.reveal && active.item == Some(msg.id),
                        rise,
                        reduced: motion.reduced,
                        gif_autoplay,
                        quiet,
                        lock,
                    };
                    message(
                        &mut col, state, env, snap, msg, &messages, &ctx, &mut acts, &mut owner,
                    );
                    after_line = false;
                    prev = Some(msg);
                }
                let h = col.min_rect().height();
                ui.allocate_space(vec2(ui.available_width(), h));
            });
        if settle {
            let end = ui.min_rect().bottom();
            let left = ui.min_rect().left();
            ui.scroll_to_rect(
                Rect::from_min_size(pos2(left, end - 1.0), vec2(1.0, 1.0)),
                Some(Align::BOTTOM),
            );
        }
    });
    active.reveal = false;
    // Scrolled by anything (wheel, bar, keys, a smooth scroll still moving): quiet.
    let offset = out.state.clamped_offset().y;
    if (offset - gate.offset).abs() > 0.5 {
        gate.quiet_since = Some(now_s);
    }
    gate.offset = offset;
    gate.owner = if quiet { None } else { owner };
    ui.ctx().data_mut(|d| d.insert_temp(gate_key, gate));
    let log = out.inner_rect;
    let node = ui.interact(log, key.with("log"), Sense::hover());
    crate::a11y::labelled(ui.ctx(), node.id, egui::Role::Log, &s::transcript(name));
    // What the rows asked for.
    for a in acts {
        apply(ui, state, env, snap, chat, &messages, a, &mut active);
    }
    ui.ctx().data_mut(|d| d.insert_temp(active_key, active));
    message_menu(ui, state, env, snap, chat, &messages);
    let Some(conv) = state.chat.open.as_mut() else {
        return;
    };
    let was_settled = conv.settled;
    if settle {
        conv.settled = true;
        ui.ctx().request_repaint();
    }
    let prev_h: Option<f32> = ui.ctx().data(|d| d.get_temp(key.with("h")));
    ui.ctx()
        .data_mut(|d| d.insert_temp(key.with("h"), out.content_size.y));
    let just_prepended = conv.prepended;
    if conv.prepended {
        conv.prepended = false;
        if let Some(h0) = prev_h {
            let grown = out.content_size.y - h0;
            ui.ctx().data_mut(|d| {
                d.insert_temp(key.with("shift"), out.state.clamped_offset().y + grown)
            });
            ui.ctx().request_repaint();
        }
    }
    if out.state.clamped_offset().y <= m::OLDER_AT
        && was_settled
        && !just_prepended
        && out.content_size.y > out.inner_rect.height()
        && let Some(before) = state.chat.wants_older()
    {
        let q = env.bridge.query_id();
        state.chat.asked(q, true);
        env.bridge.send(UiCommand::Chat(ChatCommand::LoadChat {
            chat,
            before: Some(before),
            reply_to: q,
        }));
    }
}

fn conv_loaded(state: &UiState) -> bool {
    state.chat.open.as_ref().is_some_and(|c| c.loaded)
}

/// Whether `b` continues `a`'s run: the same person, within six minutes, both written.
fn continues(a: &ChatMessage, b: &ChatMessage) -> bool {
    a.author == b.author
        && !is_sys(a)
        && !is_sys(b)
        && !matches!(a.body, MessageBody::Stream(_))
        && !matches!(b.body, MessageBody::Stream(_))
        && (b.at.as_millis() - a.at.as_millis()) <= m::RUN_MS
}

/// The start: the chat's name in display 26 and what end-to-end encryption means here.
fn start_block(ui: &mut Ui, name: &str, group: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = (ui.available_width() - m::INDENT).clamp(1.0, m::START_MAX);
    let top = ui.cursor().top() + m::START_TOP;
    let left = ui.max_rect().left() + m::INDENT;
    let flip = Flip::new(ui.max_rect());
    let title = Type::Title.galley(ui, name, p.t1, w, usize::MAX);
    let body = if group {
        format!("{} {}", s::START_GROUP(), s::ENCRYPTED_GROUP())
    } else {
        format!("{} {}", s::start_dm(&first(name)), s::ENCRYPTED_DM())
    };
    let para = super::paint::Para::new(ui, &body, Type::Body, p.t3, w);
    let size = para.size();
    // Lines of 19 rather than the token's 18.
    let lines = para.lines.len() as f32;
    let body_h = size.y + lines * (m::START_LINE - Type::Body.line_height());
    let tr = flip.r(Rect::from_min_size(pos2(left, top), title.size()));
    let th = title.size().y;
    ui.painter().galley(tr.min, title, p.t1);
    let by = top + th + m::START_GAP;
    let mut y = by;
    let rtl = para.rtl || flip.rtl;
    for g in para.lines {
        let x = if rtl { left + w - g.size().x } else { left };
        let r = flip.r(Rect::from_min_size(pos2(x, y), g.size()));
        ui.painter().galley(r.min, g, p.t3);
        y += m::START_LINE;
    }
    let total = m::START_TOP + th + m::START_GAP + body_h + m::START_BOTTOM;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), total), Sense::hover());
    let node = ui.interact(r, ui.make_persistent_id("chat.start"), Sense::hover());
    crate::a11y::item(
        &node,
        egui::Role::Label,
        &crate::strings::chat::start_spoken(name, &body),
    );
}

/// A day ("Today") or "New" in accent, with a hairline running on; 22 above.
fn day_line(ui: &mut Ui, label: &str, new: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let color = if new { p.accent_text } else { p.t3 };
    let line = if new {
        palette::fade(p.accent_mark, 0.45)
    } else {
        p.line
    };
    ui.add_space(m::DAY_TOP);
    let g = Type::Label.galley(ui, label, color, f32::INFINITY, 1);
    let h = Type::Label.line_height();
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let flip = Flip::new(r);
    let lx = r.left() + m::INDENT;
    let gw = g.size().x;
    let gr = flip.r(Rect::from_min_size(pos2(lx, r.top()), g.size()));
    ui.painter().galley(gr.min, g, color);
    let a = flip.p(pos2(lx + gw + m::DAY_GAP, r.center().y));
    let b = flip.p(pos2(r.right(), r.center().y));
    ui.painter().hline(
        a.x.min(b.x)..=a.x.max(b.x),
        r.center().y.round() + 0.5,
        Stroke::new(1.0, line),
    );
    crate::a11y::item(&resp, egui::Role::Label, label);
    ui.add_space(m::DAY_BOTTOM);
}

/// Grey shapes where messages will be.
fn skeleton_messages(ui: &mut Ui) {
    let w = ui.available_width();
    for (k, share) in [0.5, 0.35, 0.6, 0.3].into_iter().enumerate() {
        ui.add_space(m::HEAD_TOP);
        let (rect, _) = ui.allocate_exact_size(vec2(w, m::AVATAR), Sense::hover());
        let av = Rect::from_min_size(rect.min, vec2(m::AVATAR, m::AVATAR));
        crate::widgets::skeleton::block_at(
            ui,
            av,
            Some(CornerRadius::same(theme::tokens::radius::PILL)),
            k,
        );
        crate::widgets::skeleton::line(
            ui,
            pos2(rect.left() + m::INDENT, rect.top() + 4.0),
            (w - m::INDENT) * share,
            10.0,
        );
        crate::widgets::skeleton::line(
            ui,
            pos2(rect.left() + m::INDENT, rect.top() + 22.0),
            (w - m::INDENT) * share * 1.4,
            10.0,
        );
    }
}

// ---- Messages -------------------------------------------------------------------------------

/// Where a row stands among the others.
struct RowCtx {
    chat: ChatRef,
    /// The first of a run: avatar, name and time.
    head: bool,
    /// Right under a day line or "New" (no 12 above).
    after_line: bool,
    /// The keys' message.
    active: bool,
    /// Bring it into view.
    reveal: bool,
    /// A new message rising: its progress 0 → 1.
    rise: Option<f32>,
    reduced: bool,
    /// GIFs play by themselves (Settings › Chat).
    gif_autoplay: bool,
    /// The transcript scrolls: no hover fill, tools or gutter time.
    quiet: bool,
    /// Only this message may count as pointed at (the pointer is on its tools).
    lock: Option<i64>,
}

/// Seconds after scrolling stops before rows light up again (on the next pointer move).
const SCROLL_GRACE: f64 = 0.2;

/// What the pointer may light up in the transcript, kept between passes.
#[derive(Clone, Copy, Default)]
struct Pointing {
    /// Scrolling made the rows quiet at this time; cleared by a pointer move after the grace.
    quiet_since: Option<f64>,
    /// The scroll offset last pass.
    offset: f32,
    /// The message whose tools showed last pass, and where they stood.
    owner: Option<(i64, Rect)>,
}

/// Something a row asked for, done after the transcript is drawn.
enum Act {
    React {
        item: i64,
        emoji: String,
        on: bool,
    },
    Reply(i64),
    Edit(i64),
    Delete(i64),
    Menu(MenuAt),
    Picker {
        item: i64,
        anchor: Rect,
    },
    Spoiler(i64, u32),
    Reveal(i64),
    Link(String),
    SaveGif(String),
    Voice(VoicePlay),
    Watch,
    StopWatch,
    StopShare,
    CallBack,
    Activate(i64, bool),
    /// Something asked of a picture, video or file.
    Media(i64, super::media::Act),
}

/// One row: a line that happened, a collapsed message, or a message with its parts.
#[allow(clippy::too_many_arguments)]
fn message(
    ui: &mut Ui,
    state: &UiState,
    env: &Env<'_>,
    snap: &ChatSnapshot,
    msg: &ChatMessage,
    all: &[ChatMessage],
    rc: &RowCtx,
    acts: &mut Vec<Act>,
    owner: &mut Option<(i64, Rect)>,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let f = snap.features;
    let author = who(snap, rc.chat, &msg.author);
    let when = crate::strings::stats::clock_time(msg.at, env.utc_offset_s);
    let hidden = author.blocked && !state.chat.revealed.contains(&(msg.id, u32::MAX));
    let sys = is_sys(msg) || hidden;
    let head = rc.head && !sys;
    if head && !rc.after_line {
        ui.add_space(m::HEAD_TOP);
    }
    let w = ui.available_width();
    let top = ui.cursor().top();
    let col_left = ui.max_rect().left();
    let (alpha, dy) = match rc.rise {
        Some(t) => (
            t,
            if rc.reduced {
                0.0
            } else {
                // Whole points: a child placed between pixels is drawn blurred (and debug
                // builds flag it "Unaligned").
                (m::RISE * (1.0 - ease_out(t))).round()
            },
        ),
        None => (1.0, 0.0),
    };
    // The plate reaches 8 past the column on each side.
    let plate_left = col_left - m::OVERHANG;
    let plate_w = w + m::OVERHANG * 2.0;
    let row_id = Id::unique(("chat.msg", rc.chat, msg.id));
    let menu_open = ui
        .ctx()
        .data(|d| d.get_temp::<MenuAt>(menu_key()))
        .is_some_and(|mm| mm.item == msg.id);
    let sense = if rc.active {
        Sense::click()
    } else {
        Sense::CLICK
    };
    let spoken = spoken_of(state, snap, rc.chat, msg, &author, &when);
    let mut row = ui.new_child(
        UiBuilder::new()
            .scope_id(row_id)
            .max_rect(Rect::from_min_size(
                pos2(plate_left, top + dy),
                vec2(plate_w, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min))
            .sense(sense)
            .accessibility_role(egui::Role::ListItem)
            .accessibility_label(spoken),
    );
    row.multiply_opacity(alpha);
    row.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let resp = row.response();
    let flip = Flip::new(Rect::from_min_size(
        pos2(plate_left, top),
        vec2(plate_w, 1.0),
    ));
    let plate_shape = row.painter().add(Shape::Noop);
    let pad_top = if head { m::HEAD_PAD_TOP } else { m::ROW_PAD };
    let body_left = col_left + m::INDENT;
    let body_w = (w - m::INDENT).max(1.0);
    let body_rect = flip.r(Rect::from_min_size(
        pos2(body_left, top + dy + pad_top),
        vec2(body_w, 1.0),
    ));
    let mut body = row.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                body_rect.min,
                vec2(body_w, f32::INFINITY),
            ))
            .layout(Layout::top_down(if flip.rtl {
                Align::Max
            } else {
                Align::Min
            })),
    );
    body.spacing_mut().item_spacing = vec2(0.0, 0.0);
    // The text above the row takes the pointer (it is selectable): the row counts as
    // pointed at while the pointer is anywhere over it.
    let pointer_on = !rc.quiet
        && match rc.lock {
            Some(item) => item == msg.id,
            None => resp.contains_pointer(),
        };
    let hovered = pointer_on || resp.has_focus() || menu_open;
    // The first column: avatar, a line's glyph, or the gutter's time.
    let gutter = flip.r(Rect::from_min_size(
        pos2(col_left, top + dy + pad_top),
        vec2(m::AVATAR, m::AVATAR),
    ));
    if hidden {
        sys_line(
            &mut body,
            &p,
            icons::PROHIBIT,
            p.t3,
            gutter,
            &[(s::HIDDEN().to_owned(), p.t3, false)],
            Some((s::SHOW(), Act::Reveal(msg.id))),
            acts,
        );
    } else if let MessageBody::Call(rec) = &msg.body {
        let (text, glyph) = match rec.kind {
            CallKind::Ended => (
                s::call_ended(&crate::strings::fmt::play_time(
                    u64::from(rec.minutes) * 60_000,
                )),
                p.t3,
            ),
            CallKind::Missed => (s::missed_from(&author.short), p.coral),
            CallKind::Declined => (s::CALL_DECLINED().to_owned(), p.t3),
        };
        let back = (rec.kind == CallKind::Missed && f.calls && !author.mine)
            .then(|| (s::CALL_BACK(), Act::CallBack));
        sys_line(
            &mut body,
            &p,
            icons::PHONE,
            glyph,
            gutter,
            &[(text, p.t2, false), (when.clone(), p.t3, true)],
            back,
            acts,
        );
    } else if let MessageBody::Notice(n) = &msg.body {
        let t = s::notice(n, &friend_name(snap, rc.chat));
        sys_line(
            &mut body,
            &p,
            icons::INFO,
            p.t3,
            gutter,
            &[(t.clone(), p.t3, false), (when.clone(), p.t3, true)],
            None,
            acts,
        );
    } else {
        if head {
            avatar::avatar(
                row.painter(),
                gutter,
                &author.name,
                author.paper,
                AvShape::Person,
            );
            name_row(&mut body, &p, &author.short, &when);
        } else if hovered {
            let size = FontId::new(m::GUTTER_SIZE, Type::Micro.numeric_font_id().family);
            let x = col_left + m::AVATAR;
            let at = flip.p(pos2(x, top + dy + pad_top + m::GUTTER_LINE / 2.0));
            row.painter().text(
                at,
                if flip.rtl {
                    Align2::LEFT_CENTER
                } else {
                    Align2::RIGHT_CENTER
                },
                &when,
                size,
                p.t3,
            );
        }
        if let Some(q) = &msg.reply
            && f.replies
        {
            reply_line(&mut body, &p, snap, rc.chat, q);
        }
        message_body(&mut body, state, snap, msg, &author, rc, acts);
        if f.reactions && !msg.reactions.is_empty() {
            reactions(&mut body, state, msg, rc, acts);
        }
        note(&mut body, &p, snap, rc.chat, msg);
    }
    let body_h = body
        .min_rect()
        .height()
        .max(if head { m::AVATAR } else { 0.0 });
    let row_h = if sys {
        body_h.max(m::SYS_H - m::ROW_PAD * 2.0) + m::ROW_PAD * 2.0
    } else {
        pad_top + body_h + m::ROW_PAD
    };
    drop(body);
    let plate = Rect::from_min_size(pos2(plate_left, top + dy), vec2(plate_w, row_h));
    row.expand_to_include_rect(plate);
    // Hover o1, radius 8.
    let hover = row.ctx().animate_bool_with_time(
        row_id.with("hover"),
        hovered,
        theme.motion.fade(Dur::Fast),
    );
    let focus = resp.has_focus();
    let fill = palette::fade(p.o1, hover.max(if focus { 1.0 } else { 0.0 }));
    row.painter().set(
        plate_shape,
        Shape::rect_filled(plate, CornerRadius::same(m::ROW_R), fill),
    );
    if focus && crate::widgets::focus_visible(row.ctx()) {
        crate::widgets::focus_ring(
            row.painter(),
            plate.shrink(2.0),
            CornerRadius::same(m::ROW_R),
            &theme,
        );
    }
    // The tools over the row's top right.
    let tools_r = tools_rect(&row, state, snap, msg, sys, plate, flip);
    if let Some(tr) = tools_r
        && hovered
        && !sys
    {
        tools(&mut row, state, snap, msg, &author, tr, acts);
        if pointer_on {
            *owner = Some((msg.id, tr));
        }
    }
    if rc.active {
        crate::widgets::own_arrow_keys(&resp);
    }
    if rc.reveal {
        resp.scroll_to_me(None);
    }
    if resp.secondary_clicked() {
        let at = resp.interact_pointer_pos().unwrap_or(plate.center());
        acts.push(Act::Menu(MenuAt {
            item: msg.id,
            anchor: Rect::from_min_size(at, vec2(1.0, 1.0)),
        }));
    }
    if resp.clicked() || resp.secondary_clicked() {
        acts.push(Act::Activate(msg.id, false));
    }
    if focus {
        keys(&row, state, snap, msg, all, plate, acts);
    }
    drop(row);
    ui.allocate_exact_size(vec2(w, row_h), Sense::hover());
}

/// The friend of a direct chat, by name (empty in a group).
fn friend_name(snap: &ChatSnapshot, chat: ChatRef) -> String {
    match chat {
        ChatRef::Direct(f) => snap.friend(f).map(|f| first(&f.name)).unwrap_or_default(),
        ChatRef::Group(_) => String::new(),
    }
}

/// What a screen reader says for a message.
fn spoken_of(
    state: &UiState,
    snap: &ChatSnapshot,
    chat: ChatRef,
    msg: &ChatMessage,
    author: &Who,
    when: &str,
) -> String {
    let text = match &msg.body {
        MessageBody::Text(t) => {
            let names: Vec<&str> = msg.mentions.iter().map(|m| m.name.as_str()).collect();
            let blocks = markdown::parse(t, &names);
            markdown::spoken(&blocks, s::SPOILER_WORD(), |n| {
                state.chat.revealed.contains(&(msg.id, n))
            })
        }
        MessageBody::Notice(n) => s::notice(n, &friend_name(snap, chat)),
        MessageBody::Voice { secs, .. } => s::voice_spoken(&mmss(*secs as f32)),
        MessageBody::Gif(n) => s::gif_spoken(&crate::demo_chat::gif_word(n).to_lowercase()),
        MessageBody::Image(m) => s::photo_spoken(&s::size(m.size)),
        MessageBody::Video(m) => s::video_spoken(
            &m.duration.map(|d| mmss(d as f32)).unwrap_or_default(),
            &s::size(m.size),
        ),
        MessageBody::File(m) => s::file_spoken(&m.name, &s::size(m.size)),
        MessageBody::Call(rec) => match rec.kind {
            CallKind::Ended => s::call_ended(&crate::strings::fmt::play_time(
                u64::from(rec.minutes) * 60_000,
            )),
            CallKind::Missed => s::missed_from(&author.short),
            CallKind::Declined => s::CALL_DECLINED().to_owned(),
        },
        MessageBody::Stream(st) => stream_title(st, author),
        MessageBody::Deleted => s::DELETED().to_owned(),
    };
    let _ = (snap, chat);
    let text = if author.blocked && !state.chat.revealed.contains(&(msg.id, u32::MAX)) {
        s::HIDDEN().to_owned()
    } else {
        text
    };
    s::message_spoken(&author.short, when, &text)
}

fn stream_title(st: &StreamRecord, author: &Who) -> String {
    match (st.live, author.mine) {
        (true, true) => s::you_sharing(&st.title),
        (true, false) => s::sharing(&author.short, &st.title),
        (false, true) => s::you_shared(&st.title),
        (false, false) => s::shared(&author.short, &st.title),
    }
}

/// `.m-n`: the first name in 13 semibold and the time in 11 quiet, on one 18 line.
fn name_row(ui: &mut Ui, p: &theme::Palette, name: &str, when: &str) {
    let ng = Type::BodyBold.galley(ui, name, p.t1, ui.available_width(), 1);
    let tg = Type::Micro.galley(ui, when, p.t3, f32::INFINITY, 1);
    let w = ng.size().x + m::NAME_GAP + tg.size().x;
    let (r, _) = ui.allocate_exact_size(vec2(w, m::NAME_H), Sense::hover());
    let flip = Flip::new(r);
    let nb = galley_baseline(&ng);
    let tb = galley_baseline(&tg);
    let base = r.top() + (m::NAME_H - ng.size().y) / 2.0 + nb;
    let nw = ng.size().x;
    let nr = flip.r(Rect::from_min_size(pos2(r.left(), base - nb), ng.size()));
    ui.painter().galley(nr.min, ng, p.t1);
    let tr = flip.r(Rect::from_min_size(
        pos2(r.left() + nw + m::NAME_GAP, base - tb),
        tg.size(),
    ));
    ui.painter().galley(tr.min, tg, p.t3);
}

fn galley_baseline(g: &egui::Galley) -> f32 {
    g.rows
        .first()
        .and_then(|r| r.row.glyphs.first().map(|gl| r.pos.y + gl.pos.y))
        .unwrap_or(g.size().y * 0.78)
}

/// `.rp`: a reply glyph · the name (t2 medium) · the first line of the message (t3).
fn reply_line(
    ui: &mut Ui,
    p: &theme::Palette,
    snap: &ChatSnapshot,
    chat: ChatRef,
    q: &easyplay_core::model::Quote,
) {
    let name = who(snap, chat, &q.author).short;
    let text = markdown::plain(&q.text, s::SPOILER_WORD());
    let max = ui.available_width().min(markdown::measure(ui));
    let ng = Type::Label.galley(ui, &name, p.t2, max, 1);
    let rest = (max - m::RP_ICON - m::RP_GAP * 2.0 - ng.size().x).max(1.0);
    let tl = markdown::OneLine::new(ui, &text, Type::Caption.font_id(), p.t3, rest);
    let w = m::RP_ICON + m::RP_GAP + ng.size().x + m::RP_GAP + tl.width();
    let (r, resp) = ui.allocate_exact_size(vec2(w.min(max), m::RP_H), Sense::hover());
    let flip = Flip::new(r);
    let cy = r.center().y;
    paint_icon(
        ui.painter(),
        flip.p(pos2(r.left() + m::RP_ICON / 2.0, cy)),
        icons::REPLY,
        m::RP_ICON,
        p.t3,
    );
    let mut x = r.left() + m::RP_ICON + m::RP_GAP;
    let nw = ng.size().x;
    let nr = flip.r(Rect::from_min_size(
        pos2(x, cy - ng.size().y / 2.0),
        ng.size(),
    ));
    ui.painter().galley(nr.min, ng, p.t2);
    x += nw + m::RP_GAP;
    let tr = flip.r(Rect::from_min_size(
        pos2(x, cy - 8.0),
        vec2(tl.width(), 16.0),
    ));
    tl.paint(ui.painter(), tr.left(), cy);
    crate::a11y::describe(&resp, &s::reply_spoken(&name, &text));
    ui.add_space(m::RP_BELOW);
}

/// What the message is: text, a voice message, a GIF, a shared game, or "deleted".
fn message_body(
    ui: &mut Ui,
    state: &UiState,
    snap: &ChatSnapshot,
    msg: &ChatMessage,
    author: &Who,
    rc: &RowCtx,
    acts: &mut Vec<Act>,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    match &msg.body {
        MessageBody::Text(t) => {
            let names: Vec<&str> = if snap.features.mentions && matches!(rc.chat, ChatRef::Group(_))
            {
                msg.mentions.iter().map(|m| m.name.as_str()).collect()
            } else {
                Vec::new()
            };
            let mut blocks = markdown::parse(t, &names);
            if msg.edited {
                blocks = markdown::with_edited(blocks, s::EDITED());
            }
            let waiting = msg.delivery == Delivery::Sending;
            let look = markdown::Look {
                color: if waiting { p.t2 } else { p.t1 },
                id: msg.id,
                revealed: &state.chat.revealed,
                jumbo: emoji::jumbo(t).is_some(),
                rtl: super::paint::reads_rtl(t),
                focusable: rc.active,
            };
            let clicked =
                markdown::show(ui, Id::unique(("chat.tx", rc.chat, msg.id)), &blocks, &look);
            if let Some(url) = clicked.link {
                acts.push(Act::Link(url));
            }
            if let Some(n) = clicked.secret {
                acts.push(Act::Spoiler(msg.id, n));
            }
            // A secondary click on the text opens the message's menu there.
            if let Some(at) = clicked.menu {
                acts.push(Act::Menu(MenuAt {
                    item: msg.id,
                    anchor: Rect::from_min_size(at, vec2(1.0, 1.0)),
                }));
                acts.push(Act::Activate(msg.id, false));
            }
        }
        MessageBody::Voice { secs, wave } => voice(ui, state, msg.id, *secs, wave, acts),
        // A still picture sent as a "GIF" is a picture.
        MessageBody::Gif(name) if still_picture(name).is_some() => {
            if let Some(m) = still_picture(name) {
                media_body(
                    ui,
                    rc.chat,
                    msg.id,
                    &MessageBody::Image(m),
                    author.mine,
                    acts,
                );
            }
        }
        MessageBody::Gif(name) => gif(ui, state, rc.gif_autoplay, rc.chat, msg.id, name, acts),
        MessageBody::Stream(st) => stream_card(ui, snap, rc.chat, st, author, acts),
        MessageBody::Image(_) | MessageBody::Video(_) | MessageBody::File(_) => {
            media_body(ui, rc.chat, msg.id, &msg.body, author.mine, acts);
        }
        MessageBody::Deleted => {
            let g = Type::Prose.galley(ui, s::DELETED(), p.t3, ui.available_width(), usize::MAX);
            let (r, _) = ui.allocate_exact_size(vec2(g.size().x, 21.0), Sense::hover());
            ui.painter()
                .galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, p.t3);
        }
        MessageBody::Call(_) | MessageBody::Notice(_) => {}
    }
}

/// A "GIF" whose file is a still picture (not a `.gif`): drawn as a received picture.
fn still_picture(name: &str) -> Option<easyplay_core::model::Media> {
    let path = std::path::Path::new(name);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    (path.is_absolute() && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp")).then(
        || easyplay_core::model::Media {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: 0,
            preview: None,
            duration: None,
            state: easyplay_core::model::FileState::Received(path.to_path_buf()),
        },
    )
}

/// A picture, a video or a file, and a coral line when it failed.
fn media_body(
    ui: &mut Ui,
    chat: ChatRef,
    item: i64,
    body: &MessageBody,
    mine: bool,
    acts: &mut Vec<Act>,
) {
    use easyplay_core::model::FileState;
    let key = format!("{chat:?}:{item}");
    let (act, failed) = match body {
        MessageBody::Image(m) => (
            super::media::image(ui, &key, m, mine),
            m.state == FileState::Failed,
        ),
        MessageBody::Video(m) => (
            super::media::video(ui, &key, m, mine),
            m.state == FileState::Failed,
        ),
        // A file's row says it failed itself.
        MessageBody::File(m) => (super::media::file(ui, m, mine), false),
        _ => (None, false),
    };
    if failed {
        super::media::failed_line(ui, mine);
    }
    if let Some(a) = act {
        acts.push(Act::Media(item, a));
    }
}

/// `.m-note`: a clock and "Waiting to be delivered…", or "Couldn't be sent." in coral.
fn note(ui: &mut Ui, p: &theme::Palette, snap: &ChatSnapshot, chat: ChatRef, msg: &ChatMessage) {
    if msg.author != MessageAuthor::Me {
        return;
    }
    let friend_off = match chat {
        ChatRef::Direct(f) => snap
            .friend(f)
            .is_some_and(|f| snap.features.presence && f.presence == Presence::Offline),
        ChatRef::Group(_) => false,
    };
    let (icon, text, color) = match msg.delivery {
        Delivery::Sending if snap.offline => (icons::CLOCK, s::WAITING_NET().to_owned(), p.t3),
        Delivery::Sending if friend_off => {
            let name = match chat {
                ChatRef::Direct(f) => snap.friend(f).map(|f| first(&f.name)).unwrap_or_default(),
                ChatRef::Group(_) => String::new(),
            };
            (icons::CLOCK, s::waiting_for(&name), p.t3)
        }
        Delivery::Failed => (icons::WARNING, s::NOT_SENT().to_owned(), p.coral),
        _ => return,
    };
    ui.add_space(m::NOTE_TOP);
    let g = Type::Micro.galley(
        ui,
        &text,
        color,
        ui.available_width() - m::NOTE_ICON - m::NOTE_GAP,
        1,
    );
    let w = m::NOTE_ICON + m::NOTE_GAP + g.size().x;
    let (r, resp) = ui.allocate_exact_size(vec2(w, m::NOTE_H), Sense::hover());
    let flip = Flip::new(r);
    paint_icon(
        ui.painter(),
        flip.p(pos2(r.left() + m::NOTE_ICON / 2.0, r.center().y)),
        icon,
        m::NOTE_ICON,
        color,
    );
    let gr = flip.r(Rect::from_min_size(
        pos2(
            r.left() + m::NOTE_ICON + m::NOTE_GAP,
            r.center().y - g.size().y / 2.0,
        ),
        g.size(),
    ));
    ui.painter().galley(gr.min, g, color);
    crate::a11y::item(&resp, egui::Role::Status, &text);
}

/// A line that happened: a glyph in the first column, then words, and maybe one action.
#[allow(clippy::too_many_arguments)]
fn sys_line(
    ui: &mut Ui,
    p: &theme::Palette,
    icon: Icon,
    glyph: Color32,
    gutter: Rect,
    words: &[(String, Color32, bool)],
    action: Option<(&str, Act)>,
    acts: &mut Vec<Act>,
) {
    let h = m::SYS_H - m::ROW_PAD * 2.0;
    paint_icon(
        ui.painter(),
        pos2(gutter.center().x, gutter.top() + h / 2.0),
        icon,
        icon_size::INLINE,
        glyph,
    );
    // The time and the action always fit whole: the first words give way (one line, "…").
    let link = action
        .as_ref()
        .map(|(l, _)| Type::BodyStrong.galley(ui, l, p.accent_text, f32::INFINITY, 1));
    let style = |small: bool| if small { Type::Micro } else { Type::Body };
    let rest: Vec<_> = words
        .iter()
        .skip(1)
        .map(|(t, c, small)| (style(*small).galley(ui, t, *c, f32::INFINITY, 1), *c))
        .collect();
    let rest_w: f32 = rest
        .iter()
        .map(|(g, _)| g.size().x + m::NAME_GAP)
        .sum::<f32>()
        + link.as_ref().map_or(0.0, |g| g.size().x + m::NAME_GAP);
    let first_w = (ui.available_width() - rest_w).max(1.0);
    let mut gs: Vec<_> = words
        .first()
        .map(|(t, c, small)| (style(*small).galley(ui, t, *c, first_w, 1), *c))
        .into_iter()
        .collect();
    gs.extend(rest);
    let w: f32 = gs
        .iter()
        .map(|(g, _)| g.size().x + m::NAME_GAP)
        .sum::<f32>()
        + link.as_ref().map_or(0.0, |g| g.size().x);
    let (r, _) = ui.allocate_exact_size(vec2(w.min(ui.available_width()), h), Sense::hover());
    let flip = Flip::new(r);
    let base = r.center().y + 4.5;
    let mut x = r.left();
    for (g, c) in gs {
        let b = galley_baseline(&g);
        let gw = g.size().x;
        let gr = flip.r(Rect::from_min_size(pos2(x, base - b), g.size()));
        ui.painter().galley(gr.min, g, c);
        x += gw + m::NAME_GAP;
    }
    if let (Some(g), Some((label, act))) = (link, action) {
        let b = galley_baseline(&g);
        let gr = flip.r(Rect::from_min_size(pos2(x, base - b), g.size()));
        let resp = ui.interact(
            gr.expand(2.0),
            ui.scope_id().with(("sys.act", label)),
            Sense::click(),
        );
        crate::a11y::button(&resp, label);
        ui.painter().galley(gr.min, g, p.accent_text);
        if resp.hovered() {
            ui.painter().hline(
                gr.x_range(),
                gr.top() + b + 2.0,
                Stroke::new(1.0, p.accent_text),
            );
        }
        if resp.clicked() {
            acts.push(act);
        }
    }
}

// ---- Reactions and tools ----------------------------------------------------------------------

/// Paint an emoji in `rect`: its picture, or the font's glyph.
fn paint_emoji(painter: &egui::Painter, rect: Rect, e: &str, color: Color32) {
    emoji::draw(painter, rect, e, color);
}

/// `.rx`: pills 24 high — the emoji at 15 and the count; yours tinted with an accent ring.
fn reactions(ui: &mut Ui, state: &UiState, msg: &ChatMessage, rc: &RowCtx, acts: &mut Vec<Act>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let _ = state;
    ui.add_space(m::RX_TOP);
    let max = ui.available_width();
    let sizes: Vec<(f32, std::sync::Arc<egui::Galley>)> = msg
        .reactions
        .iter()
        .map(|r| {
            let g = ui.fonts_mut(|f| {
                f.layout_job(egui::text::LayoutJob::single_section(
                    r.count.to_string(),
                    Type::Label.format(if r.mine { p.accent_text } else { p.t2 }, true),
                ))
            });
            (
                m::RX_PAD[0] + m::RX_EMOJI + m::RX_IN + g.size().x + m::RX_PAD[1],
                g,
            )
        })
        .collect();
    // Wrap into rows.
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut x = 0.0;
    for (k, (w, _)) in sizes.iter().enumerate() {
        if x > 0.0 && x + w > max {
            rows.push(Vec::new());
            x = 0.0;
        }
        x += w + m::RX_GAP;
        if let Some(r) = rows.last_mut() {
            r.push(k);
        }
    }
    for row in rows {
        let w: f32 = row.iter().map(|&k| sizes[k].0 + m::RX_GAP).sum::<f32>() - m::RX_GAP;
        let (r, _) = ui.allocate_exact_size(vec2(w.max(0.0), m::RX_H), Sense::hover());
        let flip = Flip::new(r);
        let mut x = r.left();
        for k in row {
            let (pw, g) = &sizes[k];
            let rx: &Reaction = &msg.reactions[k];
            let pill = flip.r(Rect::from_min_size(pos2(x, r.top()), vec2(*pw, m::RX_H)));
            x += pw + m::RX_GAP;
            let resp = ui.interact(
                pill,
                Id::unique(("chat.rx", rc.chat, msg.id, &rx.emoji)),
                if rc.active {
                    Sense::click()
                } else {
                    Sense::CLICK
                },
            );
            let label = s::reaction_spoken(&rx.emoji, rx.count, rx.mine);
            crate::a11y::toggle(&resp, &label, rx.mine);
            let hover = crate::widgets::hover_t(&resp, &theme);
            // The one you add pops from .7.
            let pop = motion::since_change(
                ui.ctx(),
                resp.id.with("pop"),
                rx.mine,
                if rc.reduced { 0.0 } else { Dur::Slow.secs() },
                false,
            )
            .filter(|_| rx.mine)
            .map_or(1.0, |t| {
                m::POP_FROM + (1.0 - m::POP_FROM) * ease_pop(t / Dur::Slow.secs())
            });
            let shown = Rect::from_center_size(pill.center(), pill.size() * pop);
            let corner = CornerRadius::same(theme::tokens::radius::PILL);
            let fill = if rx.mine {
                p.tint
            } else {
                mix(p.o1, p.o2, hover)
            };
            ui.painter().rect_filled(shown, corner, fill);
            if rx.mine {
                ui.painter().rect_stroke(
                    shown,
                    corner,
                    Stroke::new(1.0, palette::fade(p.accent_mark, 0.5)),
                    StrokeKind::Inside,
                );
            }
            let lp = Rect::from_min_size(pos2(pill.left(), pill.top()), pill.size());
            let inner = Flip::new(lp);
            let er = inner.r(Rect::from_min_size(
                pos2(lp.left() + m::RX_PAD[0], lp.center().y - m::RX_EMOJI / 2.0),
                vec2(m::RX_EMOJI, m::RX_EMOJI) * pop,
            ));
            paint_emoji(ui.painter(), er, &rx.emoji, p.t1);
            let gr = inner.r(Rect::from_min_size(
                pos2(
                    lp.left() + m::RX_PAD[0] + m::RX_EMOJI + m::RX_IN,
                    lp.center().y - g.size().y / 2.0,
                ),
                g.size(),
            ));
            ui.painter().galley(gr.min, g.clone(), p.t2);
            if resp.has_focus() {
                crate::widgets::focus_ring(ui.painter(), pill, corner, &theme);
            }
            if rx.who.is_empty() {
                tooltip::attach(resp.clone(), &label);
            } else {
                let names: Vec<&str> = rx.who.iter().map(String::as_str).collect();
                tooltip::attach(
                    resp.clone(),
                    &crate::strings::chat::details::and_list(&names),
                );
            }
            if resp.clicked() {
                acts.push(Act::React {
                    item: msg.id,
                    emoji: rx.emoji.clone(),
                    on: !rx.mine,
                });
            }
        }
        ui.add_space(m::RX_GAP);
    }
    ui.add_space(m::RX_BOTTOM - m::RX_GAP);
}

/// What the tools capsule holds.
struct ToolSet {
    emoji: Vec<String>,
    react: bool,
    reply: bool,
}

fn tool_set(state: &UiState, snap: &ChatSnapshot, msg: &ChatMessage) -> ToolSet {
    let f = snap.features;
    let writable = !matches!(msg.body, MessageBody::Deleted);
    ToolSet {
        emoji: if f.reactions && writable {
            emoji::quick(&state.chat.freq, snap.reaction_emoji.as_deref(), 3)
        } else {
            Vec::new()
        },
        react: f.reactions && writable,
        reply: f.replies && writable,
    }
}

/// Where the capsule stands: 8 in from the row's right, 16 above it.
fn tools_rect(
    _ui: &Ui,
    state: &UiState,
    snap: &ChatSnapshot,
    msg: &ChatMessage,
    sys: bool,
    plate: Rect,
    flip: Flip,
) -> Option<Rect> {
    if sys {
        return None;
    }
    let t = tool_set(state, snap, msg);
    let n = t.emoji.len() + usize::from(t.react) + usize::from(t.reply) + 1;
    let rule = if t.emoji.is_empty() {
        0.0
    } else {
        1.0 + m::TOOL_RULE_M * 2.0
    };
    let w = m::TOOLS_PAD * 2.0 + n as f32 * m::TOOL + rule;
    let r = Rect::from_min_size(
        pos2(plate.right() - m::TOOLS_IN - w, plate.top() - m::TOOLS_UP),
        vec2(w, m::TOOLS_H),
    );
    Some(flip.r(r))
}

/// `.mt`: the raised capsule — three emoji · add reaction · reply · more.
fn tools(
    ui: &mut Ui,
    state: &UiState,
    snap: &ChatSnapshot,
    msg: &ChatMessage,
    author: &Who,
    rect: Rect,
    acts: &mut Vec<Act>,
) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let t = tool_set(state, snap, msg);
    let _ = author;
    let painter = ui.painter().clone();
    let corner = CornerRadius::same(m::TOOLS_R);
    // The capsule takes the pointer, so the row above it does not light up.
    let _bg = ui.interact(rect, ui.scope_id().with("tools.bg"), Sense::click());
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 4],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(56),
        }
        .as_shape(rect, corner),
    );
    painter.rect_filled(rect, corner, p.raised);
    painter.rect_stroke(rect, corner, Stroke::new(1.0, p.line), StrokeKind::Inside);
    let flip = Flip::new(rect);
    let mut x = rect.left() + m::TOOLS_PAD;
    let cy = rect.center().y;
    let button = |ui: &mut Ui, x: &mut f32, key: &str, label: &str| {
        let r = flip.r(Rect::from_min_size(
            pos2(*x, cy - m::TOOL / 2.0),
            vec2(m::TOOL, m::TOOL),
        ));
        *x += m::TOOL;
        let resp = ui.interact(r, Id::unique(("chat.tool", msg.id, key)), Sense::CLICK);
        crate::a11y::button(&resp, label);
        let hover = crate::widgets::hover_t(&resp, &theme);
        painter.rect_filled(r, CornerRadius::same(m::TOOL_R), palette::fade(p.o2, hover));
        (resp, mix(p.t2, p.t1, hover))
    };
    for e in &t.emoji {
        let name = emoji::name_of(e).map_or_else(|| e.clone(), |n| n.replace('_', " "));
        let (resp, _) = button(ui, &mut x, e, &s::react_with(&name));
        let er = Rect::from_center_size(resp.rect.center(), vec2(m::TOOL_EMOJI, m::TOOL_EMOJI));
        paint_emoji(&painter, er, e, p.t1);
        if resp.clicked() {
            let mine = msg.reactions.iter().any(|r| r.emoji == *e && r.mine);
            acts.push(Act::React {
                item: msg.id,
                emoji: e.clone(),
                on: !mine,
            });
        }
    }
    if !t.emoji.is_empty() {
        x += m::TOOL_RULE_M;
        let rr = flip.r(Rect::from_min_size(
            pos2(x, cy - m::TOOL_RULE / 2.0),
            vec2(1.0, m::TOOL_RULE),
        ));
        painter.rect_filled(rr, CornerRadius::ZERO, p.line);
        x += 1.0 + m::TOOL_RULE_M;
    }
    if t.react {
        let (resp, color) = button(ui, &mut x, "react", s::ADD_REACTION());
        // "smile-plus": the smiley with a small plus at its top right.
        let c = resp.rect.center();
        paint_icon(
            &painter,
            c + vec2(-1.0, 1.0),
            icons::SMILEY,
            icon_size::INLINE - 2.0,
            color,
        );
        let pc = c + vec2(6.0, -5.5);
        painter.circle_filled(pc, 4.0, p.raised);
        painter.hline(pc.x - 2.5..=pc.x + 2.5, pc.y, Stroke::new(1.3, color));
        painter.vline(pc.x, pc.y - 2.5..=pc.y + 2.5, Stroke::new(1.3, color));
        tooltip::attach(resp.clone(), s::ADD_REACTION());
        if resp.clicked() {
            acts.push(Act::Picker {
                item: msg.id,
                anchor: resp.rect,
            });
        }
    }
    if t.reply {
        let (resp, color) = button(ui, &mut x, "reply", s::REPLY());
        paint_icon(
            &painter,
            resp.rect.center(),
            icons::REPLY,
            icon_size::INLINE,
            color,
        );
        tooltip::tooltip(resp.clone(), s::REPLY(), Some("R"));
        if resp.clicked() {
            acts.push(Act::Reply(msg.id));
        }
    }
    let (resp, color) = button(ui, &mut x, "more", s::MORE());
    paint_icon(
        &painter,
        resp.rect.center(),
        icons::DOTS,
        icon_size::INLINE,
        color,
    );
    tooltip::attach(resp.clone(), s::MORE());
    if resp.clicked() {
        acts.push(Act::Menu(MenuAt {
            item: msg.id,
            anchor: resp.rect,
        }));
    }
}

// ---- Voice, GIF, shared game -----------------------------------------------------------------

fn mmss(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Bar heights (0–1) of a voice message: its own waveform resampled, or one made from the
/// message's number when it has none.
fn wave_bars(seed: i64, secs: u32, wave: &[u8]) -> Vec<f32> {
    let n = ((secs as f32 * m::BARS_PER_S).round() as usize).clamp(m::BARS[0], m::BARS[1]);
    if !wave.is_empty() {
        return (0..n)
            .map(|i| {
                let k = i * wave.len() / n;
                (f32::from(wave[k.min(wave.len() - 1)]) / 255.0).max(0.12)
            })
            .collect();
    }
    let mut x = (seed as u64).wrapping_mul(977).wrapping_add(13) | 1;
    (0..n)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let r = (x % 1000) as f32 / 1000.0;
            let s = ((i as f32) * 0.5 + seed as f32).sin().abs();
            0.16 + 0.84 * s * (0.3 + 0.7 * r)
        })
        .collect()
}

/// `.vm`: play / pause · the waveform (click to jump) · the length · speed.
fn voice(ui: &mut Ui, state: &UiState, item: i64, secs: u32, wave: &[u8], acts: &mut Vec<Act>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let bars = wave_bars(item, secs, wave);
    let wave_w = bars.len() as f32 * (m::BAR_W + m::BAR_GAP) - m::BAR_GAP;
    let w = m::VM_PAD
        + m::VM_PLAY
        + m::VM_GAP
        + wave_w
        + m::VM_GAP
        + m::VM_TIME_W
        + m::VM_GAP
        + m::VM_SPEED[0]
        + m::VM_PAD;
    ui.add_space(m::VM_MARGIN);
    let (r, _) = ui.allocate_exact_size(vec2(w.min(ui.available_width()), m::VM_H), Sense::hover());
    let flip = Flip::new(r);
    let corner = CornerRadius::same(theme::tokens::radius::PILL);
    ui.painter().rect_filled(r, corner, p.o1);
    if p.outlined() {
        ui.painter()
            .rect_stroke(r, corner, Stroke::new(1.0, p.line), StrokeKind::Inside);
    }
    let play = state.chat.voice.filter(|v| v.item == item);
    let playing = play.is_some_and(|v| !v.paused);
    let speed = play.map_or(1.0, |v| v.speed);
    let pos = play.map_or(0.0, |v| v.pos);
    let cy = r.center().y;
    let mut x = r.left() + m::VM_PAD;
    // Play / pause: a t1 disc with a bg glyph.
    let pr = flip.r(Rect::from_min_size(
        pos2(x, cy - m::VM_PLAY / 2.0),
        vec2(m::VM_PLAY, m::VM_PLAY),
    ));
    let label = if playing { s::PAUSE() } else { s::PLAY() };
    let resp = ui.interact(pr, Id::unique(("chat.vm.play", item)), Sense::click());
    crate::a11y::button(&resp, &format!("{label}, {}", mmss(secs as f32)));
    let press = crate::widgets::press_t(&resp, &theme);
    let shown = Rect::from_center_size(pr.center(), pr.size() * (1.0 - 0.06 * press));
    ui.painter()
        .circle_filled(shown.center(), shown.width() / 2.0, p.t1);
    paint_icon(
        ui.painter(),
        pr.center() + vec2(if playing { 0.0 } else { 1.0 }, 0.0),
        if playing { icons::PAUSE } else { icons::PLAY },
        m::VM_ICON,
        p.bg,
    );
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), pr, corner, &theme);
    }
    if resp.clicked() {
        acts.push(Act::Voice(match play {
            Some(v) => VoicePlay {
                paused: !v.paused,
                ..v
            },
            None => VoicePlay {
                item,
                pos: 0.0,
                speed: 1.0,
                paused: false,
            },
        }));
    }
    x += m::VM_PLAY + m::VM_GAP;
    // The waveform.
    let wr = Rect::from_min_size(pos2(x, cy - m::WAVE_H / 2.0), vec2(wave_w, m::WAVE_H));
    let fwr = flip.r(wr);
    let wresp = ui.interact(fwr, Id::unique(("chat.vm.wave", item)), Sense::CLICK);
    let on = if secs > 0 {
        ((pos / secs as f32) * bars.len() as f32).round() as usize
    } else {
        0
    };
    for (i, v) in bars.iter().enumerate() {
        let h = (v * m::WAVE_H).max(m::BAR_MIN);
        let bx = x + i as f32 * (m::BAR_W + m::BAR_GAP);
        let br = flip.r(Rect::from_min_size(
            pos2(bx, cy - h / 2.0),
            vec2(m::BAR_W, h),
        ));
        let color = if play.is_some() && i < on { p.t1 } else { p.t4 };
        ui.painter().rect_filled(br, CornerRadius::same(1), color);
    }
    if wresp.clicked()
        && let Some(at) = wresp.interact_pointer_pos()
    {
        let frac = ((at.x - fwr.left()) / fwr.width()).clamp(0.0, 1.0);
        let frac = if flip.rtl { 1.0 - frac } else { frac };
        acts.push(Act::Voice(VoicePlay {
            item,
            pos: (frac * secs as f32).max(0.01),
            speed,
            paused: false,
        }));
    }
    x += wave_w + m::VM_GAP;
    // The length (where it is, while playing).
    let shown_t = if play.is_some() { pos } else { secs as f32 };
    let tg = ui.fonts_mut(|f| {
        f.layout_job(egui::text::LayoutJob::single_section(
            mmss(shown_t),
            Type::Caption.format(p.t2, true),
        ))
    });
    let tr = flip.r(Rect::from_min_size(
        pos2(x, cy - tg.size().y / 2.0),
        vec2(m::VM_TIME_W, tg.size().y),
    ));
    let tx = if flip.rtl {
        tr.right() - tg.size().x
    } else {
        tr.left()
    };
    ui.painter().galley(pos2(tx, tr.top()), tg, p.t2);
    x += m::VM_TIME_W + m::VM_GAP;
    // Speed: 1× → 1.5× → 2×.
    let sr = flip.r(Rect::from_min_size(
        pos2(x, cy - m::VM_SPEED[1] / 2.0),
        vec2(m::VM_SPEED[0], m::VM_SPEED[1]),
    ));
    let sresp = ui.interact(sr, Id::unique(("chat.vm.speed", item)), Sense::click());
    crate::a11y::button(&sresp, s::SPEED());
    let hover = crate::widgets::hover_t(&sresp, &theme);
    ui.painter()
        .rect_filled(sr, corner, palette::fade(p.o2, hover));
    let word = if (speed - 1.5).abs() < 0.01 {
        "1.5×".to_owned()
    } else {
        format!("{}×", speed.round() as u32)
    };
    ui.painter().text(
        sr.center(),
        Align2::CENTER_CENTER,
        word,
        FontId::new(11.0, Type::BodyBold.font_id().family),
        mix(p.t3, p.t1, hover),
    );
    let sresp = tooltip::attach(sresp, s::SPEED());
    if sresp.clicked() {
        let next = if speed < 1.25 {
            1.5
        } else if speed < 1.75 {
            2.0
        } else {
            1.0
        };
        acts.push(Act::Voice(match play {
            Some(v) => VoicePlay { speed: next, ..v },
            None => VoicePlay {
                item,
                pos: 0.0,
                speed: next,
                paused: true,
            },
        }));
    }
    ui.add_space(m::VM_MARGIN);
}

/// `.gif`: at most 280 wide, 16:10, radius 10; a "GIF" tag; a star that saves it.
#[allow(clippy::too_many_arguments)]
fn gif(
    ui: &mut Ui,
    state: &UiState,
    autoplay: bool,
    chat: ChatRef,
    item: i64,
    name: &str,
    acts: &mut Vec<Act>,
) {
    let size = super::gif::message_size(ui.ctx(), name, ui.available_width().min(m::GIF_W));
    ui.add_space(m::GIF_MARGIN);
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    let flip = Flip::new(r);
    let word = crate::demo_chat::gif_word(name);
    let resp = super::gif::paint_message(ui, name, r, autoplay);
    // "GIF" bottom left.
    let tg = Type::MicroStrong.galley(ui, s::GIF(), Color32::WHITE, f32::INFINITY, 1);
    let tag = flip.r(Rect::from_min_size(
        pos2(r.left() + m::GIF_IN, r.bottom() - m::GIF_IN - m::GIF_TAG_H),
        vec2(tg.size().x + m::GIF_TAG_PAD * 2.0, m::GIF_TAG_H),
    ));
    ui.painter().rect_filled(
        tag,
        CornerRadius::same(m::GIF_TAG_R),
        Color32::from_black_alpha(m::GIF_SCRIM),
    );
    ui.painter().galley(
        pos2(
            tag.left() + m::GIF_TAG_PAD,
            tag.center().y - tg.size().y / 2.0,
        ),
        tg,
        Color32::WHITE,
    );
    crate::a11y::item(
        &resp,
        egui::Role::Image,
        &s::gif_spoken(&word.to_lowercase()),
    );
    // The star, top right, while pointed at (or saved).
    if snap_gifs_allowed() {
        let saved = state.chat.saved_gifs.iter().any(|g| g == name);
        let star = flip.r(Rect::from_min_size(
            pos2(r.right() - m::GIF_IN - m::GIF_STAR, r.top() + m::GIF_IN),
            vec2(m::GIF_STAR, m::GIF_STAR),
        ));
        let sresp = ui.interact(
            star,
            Id::unique(("chat.gif.star", chat, item)),
            Sense::click(),
        );
        let show = saved || ui.rect_contains_pointer(r) || sresp.has_focus();
        if show {
            ui.painter().circle_filled(
                star.center(),
                m::GIF_STAR / 2.0,
                Color32::from_black_alpha(m::GIF_SCRIM),
            );
            paint_icon(
                ui.painter(),
                star.center(),
                if saved { icons::STAR_FILL } else { icons::STAR },
                m::GIF_STAR_ICON,
                Color32::WHITE,
            );
        }
        let label = if saved {
            s::GIF_SAVED_TIP()
        } else {
            s::SAVE_GIF()
        };
        crate::a11y::toggle(&sresp, s::SAVE_GIF(), saved);
        let sresp = tooltip::attach(sresp, label);
        if sresp.clicked() {
            acts.push(Act::SaveGif(name.to_owned()));
        }
    }
    ui.add_space(m::GIF_MARGIN);
}

fn snap_gifs_allowed() -> bool {
    true
}

/// `.sc`: a shared game — the cover · "Jonas is sharing …" · "● Live  2 people watching" ·
/// Watch, Stop watching, or Stop; afterwards "Jonas shared …" and "For 54 min".
fn stream_card(
    ui: &mut Ui,
    snap: &ChatSnapshot,
    chat: ChatRef,
    st: &StreamRecord,
    author: &Who,
    acts: &mut Vec<Act>,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width().min(m::SC_W);
    let cover_h = m::SC_COVER * 1.5;
    let h = m::SC_PAD[0] + cover_h + m::SC_PAD[2];
    ui.add_space(m::SC_MARGIN);
    let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let flip = Flip::new(r);
    let corner = CornerRadius::same(m::SC_R);
    ui.painter().rect_filled(r, corner, p.o1);
    if p.outlined() {
        ui.painter()
            .rect_stroke(r, corner, Stroke::new(1.0, p.line), StrokeKind::Inside);
    }
    // The cover: the game's paper with its initials.
    let cr = flip.r(Rect::from_min_size(
        pos2(r.left() + m::SC_PAD[0], r.top() + m::SC_PAD[0]),
        vec2(m::SC_COVER, cover_h),
    ));
    let palette::Paper { paper, ink } = palette::paper_for(&st.title);
    ui.painter()
        .rect_filled(cr, crate::widgets::cover::corner(cr), paper);
    ui.painter().text(
        cr.center(),
        Align2::CENTER_CENTER,
        crate::widgets::cover::initials(&st.title),
        theme::typography::display_font(18.0),
        ink,
    );
    // The action at the right end.
    let watching_here = snap
        .live
        .watch
        .as_ref()
        .is_some_and(|wt| wt.chat == chat && wt.name == author.name);
    let action = if !st.live || !snap.features.share {
        None
    } else if author.mine {
        Some((Button::new(s::STOP()), Act::StopShare))
    } else if watching_here {
        Some((Button::new(s::STOP_WATCHING()), Act::StopWatch))
    } else {
        Some((Button::primary(s::WATCH()), Act::Watch))
    };
    let mut right = r.right() - m::SC_PAD[1];
    if let Some((b, act)) = action {
        let bw = b.width(ui);
        let br = flip.r(Rect::from_min_size(
            pos2(
                right - bw,
                r.center().y - theme::tokens::control::BUTTON / 2.0,
            ),
            vec2(bw, theme::tokens::control::BUTTON),
        ));
        if ui.put(br, b).clicked() {
            acts.push(act);
        }
        right -= bw + m::SC_GAP;
    }
    let tx = r.left() + m::SC_PAD[0] + m::SC_COVER + m::SC_GAP;
    let tw = (right - tx).max(1.0);
    let title = stream_title(st, author);
    let tg = Type::BodyStrong.galley(ui, &title, p.t1, tw, 1);
    let sub = if st.live {
        if st.watching > 0 {
            s::watching(st.watching)
        } else {
            s::NOBODY_WATCHING().to_owned()
        }
    } else {
        s::for_time(&crate::strings::fmt::play_time(
            u64::from(st.minutes) * 60_000,
        ))
    };
    let lg = st
        .live
        .then(|| Type::MicroStrong.galley(ui, s::LIVE(), p.coral, f32::INFINITY, 1));
    let live_w = lg.as_ref().map_or(0.0, |g| {
        m::LIVE_DOT + m::LIVE_GAP + g.size().x + m::NAME_GAP
    });
    let sg = Type::Caption.galley(ui, &sub, p.t3, (tw - live_w).max(1.0), 1);
    let th = tg.size().y + 1.0 + sg.size().y;
    let ty = r.center().y - th / 2.0;
    let tgh = tg.size().y;
    let tr = flip.r(Rect::from_min_size(pos2(tx, ty), tg.size()));
    ui.painter().galley(tr.min, tg, p.t1);
    let sy = ty + tgh + 1.0;
    let scy = sy + sg.size().y / 2.0;
    let mut x = tx;
    if let Some(g) = lg {
        ui.painter().circle_filled(
            flip.p(pos2(x + m::LIVE_DOT / 2.0, scy)),
            m::LIVE_DOT / 2.0,
            p.coral,
        );
        x += m::LIVE_DOT + m::LIVE_GAP;
        let gw = g.size().x;
        let lr = flip.r(Rect::from_min_size(
            pos2(x, scy - g.size().y / 2.0),
            g.size(),
        ));
        ui.painter().galley(lr.min, g, p.coral);
        x += gw + m::NAME_GAP;
    }
    let sr = flip.r(Rect::from_min_size(pos2(x, sy), sg.size()));
    ui.painter().galley(sr.min, sg, p.t3);
    ui.add_space(m::SC_MARGIN);
}

// ---- Keys, menus and what rows asked for --------------------------------------------------------

/// The keys of the focused message: ↑ ↓ Home End move, Enter opens its menu, R replies,
/// E edits, Delete deletes.
fn keys(
    ui: &Ui,
    state: &UiState,
    snap: &ChatSnapshot,
    msg: &ChatMessage,
    all: &[ChatMessage],
    plate: Rect,
    acts: &mut Vec<Act>,
) {
    let _ = state;
    let f = snap.features;
    let i = all.iter().position(|m| m.id == msg.id).unwrap_or(0);
    let pressed = |k: Key| ui.input_mut(|inp| inp.consume_key(egui::Modifiers::NONE, k));
    let to = if pressed(Key::ArrowUp) {
        Some(i.saturating_sub(1))
    } else if pressed(Key::ArrowDown) {
        Some((i + 1).min(all.len().saturating_sub(1)))
    } else if pressed(Key::Home) {
        Some(0)
    } else if pressed(Key::End) {
        Some(all.len().saturating_sub(1))
    } else {
        None
    };
    if let Some(k) = to
        && let Some(m) = all.get(k)
    {
        acts.push(Act::Activate(m.id, true));
        return;
    }
    if pressed(Key::Enter) {
        acts.push(Act::Menu(MenuAt {
            item: msg.id,
            anchor: plate,
        }));
        return;
    }
    let mine = msg.author == MessageAuthor::Me;
    let text = matches!(msg.body, MessageBody::Text(_));
    if f.replies && pressed(Key::R) {
        acts.push(Act::Reply(msg.id));
    } else if f.edit && mine && text && pressed(Key::E) {
        acts.push(Act::Edit(msg.id));
    } else if f.edit && mine && pressed(Key::Delete) {
        acts.push(Act::Delete(msg.id));
    }
}

/// Do what a row asked for.
#[allow(clippy::too_many_arguments)]
fn apply(
    ui: &Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    all: &[ChatMessage],
    act: Act,
    active: &mut Active,
) {
    let send = |env: &mut Env<'_>, c: ChatCommand| env.bridge.send(UiCommand::Chat(c));
    match act {
        Act::React { item, emoji, on } => {
            if on {
                emoji::bump(&mut state.chat.freq, &emoji);
            }
            send(
                env,
                ChatCommand::React {
                    chat,
                    item,
                    emoji,
                    on,
                },
            );
        }
        Act::Reply(item) => {
            state.chat.reply = Some(item);
            state.chat.edit = None;
            state.chat.focus_composer = true;
        }
        Act::Edit(item) => {
            if let Some(MessageBody::Text(t)) = all.iter().find(|m| m.id == item).map(|m| &m.body) {
                composer::start_edit(state, chat, item, t);
            }
        }
        Act::Delete(item) => {
            state.toast(
                s::DELETED_TOAST(),
                ToastKind::Info,
                Some(UiCommand::Chat(ChatCommand::Delete { chat, item })),
                env.mono,
            );
            if let Some(key) = state.last_undo {
                // Undo keeps the message here; nothing goes out until the toast leaves.
                state.undo.insert(key, Vec::new());
                state.chat.deleting.push(Deleting {
                    chat,
                    item,
                    undo_key: key,
                });
            }
            if state.chat.edit == Some(item) {
                state.chat.edit = None;
            }
        }
        Act::Menu(at) => {
            ui.ctx().data_mut(|d| d.insert_temp(menu_key(), at));
        }
        Act::Picker { item, anchor } => {
            state.chat.picker = Some(Picker::Emoji {
                react_to: Some(item),
            });
            set_picker_anchor(ui.ctx(), anchor);
        }
        Act::Spoiler(item, n) => {
            state.chat.revealed.insert((item, n));
        }
        Act::Reveal(item) => {
            state.chat.revealed.insert((item, u32::MAX));
        }
        Act::Link(url) => send(env, ChatCommand::OpenLink(url)),
        Act::SaveGif(name) => {
            let gifs = &mut state.chat.saved_gifs;
            let text = if let Some(i) = gifs.iter().position(|g| *g == name) {
                gifs.remove(i);
                s::GIF_REMOVED()
            } else {
                gifs.insert(0, name);
                s::GIF_SAVED()
            };
            state.toast(text, ToastKind::Success, None, env.mono);
        }
        Act::Voice(v) => state.chat.voice = Some(v),
        Act::Watch => send(env, ChatCommand::Watch(chat)),
        Act::StopWatch => send(env, ChatCommand::StopWatch),
        Act::StopShare => send(env, ChatCommand::StopShare),
        Act::CallBack => call_flow(state, env, snap, chat),
        Act::Media(item, a) => match a {
            super::media::Act::Receive => send(env, ChatCommand::ReceiveFile { chat, item }),
            super::media::Act::Cancel => send(env, ChatCommand::CancelFile { chat, item }),
            super::media::Act::Open => send(env, ChatCommand::OpenFile { chat, item }),
            super::media::Act::View => {
                state.chat.viewer = Some(crate::vm::chat::Viewer { chat, item });
            }
        },
        Act::Activate(item, focus) => {
            active.item = Some(item);
            if focus {
                active.reveal = true;
                ui.ctx()
                    .memory_mut(|mem| mem.request_focus(Id::unique(("chat.msg", chat, item))));
                ui.ctx().request_repaint();
            }
        }
    }
}

/// Where the emoji picker hangs from when it opens for a reaction.
fn set_picker_anchor(ctx: &egui::Context, anchor: Rect) {
    emoji::set_anchor(ctx, anchor);
}

/// The message menu: Reply · Add reaction… · Copy text · Save GIF · Edit · — · Delete for
/// everyone (yours) or Block <name>… (someone else's, in a group).
fn message_menu(
    ui: &Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    snap: &ChatSnapshot,
    chat: ChatRef,
    all: &[ChatMessage],
) {
    let Some(at) = ui.ctx().data(|d| d.get_temp::<MenuAt>(menu_key())) else {
        return;
    };
    let Some(msg) = all.iter().find(|m| m.id == at.item).cloned() else {
        ui.ctx().data_mut(|d| d.remove::<MenuAt>(menu_key()));
        return;
    };
    let f = snap.features;
    let author = who(snap, chat, &msg.author);
    let mine = author.mine;
    let group = matches!(chat, ChatRef::Group(_));
    let mut open = true;
    let mut chosen: Option<Act> = None;
    let mut copy: Option<String> = None;
    let mut block: Option<bool> = None;
    let popup = Popup::new(
        Id::unique(("chat.msg.menu", chat)),
        ui.ctx().clone(),
        at.anchor,
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .open_bool(&mut open)
    .align(RectAlign::BOTTOM_END)
    .close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    context_menu::popup(popup, |ui| {
        let writable = !matches!(msg.body, MessageBody::Deleted);
        if f.replies
            && writable
            && context_menu::icon_item(ui, icons::REPLY, s::REPLY(), Some("R")).clicked()
        {
            chosen = Some(Act::Reply(msg.id));
        }
        if f.reactions
            && writable
            && context_menu::icon_item(ui, icons::SMILEY, s::ADD_REACTION_MENU(), None).clicked()
        {
            chosen = Some(Act::Picker {
                item: msg.id,
                anchor: at.anchor,
            });
        }
        if let MessageBody::Text(t) = &msg.body
            && context_menu::icon_item(ui, icons::COPY, s::COPY(), None).clicked()
        {
            copy = Some(t.clone());
        }
        let received = match &msg.body {
            MessageBody::Image(m) | MessageBody::Video(m) | MessageBody::File(m) => {
                matches!(m.state, easyplay_core::model::FileState::Received(_))
            }
            _ => false,
        };
        if received && context_menu::icon_item(ui, icons::OPEN, s::OPEN(), None).clicked() {
            chosen = Some(Act::Media(msg.id, super::media::Act::Open));
        }
        if let MessageBody::Gif(n) = &msg.body {
            let saved = state.chat.saved_gifs.iter().any(|g| g == n);
            let label = if saved {
                s::UNSAVE_GIF()
            } else {
                s::SAVE_GIF()
            };
            if context_menu::icon_item(ui, icons::STAR, label, None).clicked() {
                chosen = Some(Act::SaveGif(n.clone()));
            }
        }
        let text = matches!(msg.body, MessageBody::Text(_));
        if f.edit
            && mine
            && text
            && context_menu::icon_item(ui, icons::PENCIL, s::EDIT(), Some("E")).clicked()
        {
            chosen = Some(Act::Edit(msg.id));
        }
        let can_delete = f.edit && mine && writable;
        let can_block = f.block && group && !mine && author.member.is_some();
        if can_delete || can_block {
            context_menu::separator(ui);
        }
        if can_delete
            && context_menu::icon_danger_item(ui, icons::TRASH, s::DELETE(), Some("Del")).clicked()
        {
            chosen = Some(Act::Delete(msg.id));
        }
        if can_block {
            if author.blocked {
                if context_menu::icon_item(ui, icons::PROHIBIT, &s::unblock(&author.short), None)
                    .clicked()
                {
                    block = Some(false);
                }
            } else if context_menu::icon_danger_item(
                ui,
                icons::PROHIBIT,
                &s::block(&author.short),
                None,
            )
            .clicked()
            {
                block = Some(true);
            }
        }
    });
    let done = chosen.is_some() || copy.is_some() || block.is_some();
    if !open || done {
        ui.ctx().data_mut(|d| d.remove::<MenuAt>(menu_key()));
    }
    if let Some(t) = copy {
        ui.ctx().copy_text(t);
        state.toast(s::COPIED(), ToastKind::Success, None, env.mono);
    }
    if let (Some(on), ChatRef::Group(g), Some(member)) = (block, chat, author.member) {
        match (on, author.friend) {
            (true, Some(friend)) => state.show_dialog(Dialog::Chat(ChatDialog::Block(friend))),
            _ => {
                env.bridge
                    .send(UiCommand::Chat(ChatCommand::SetMemberBlocked {
                        group: g,
                        member,
                        on,
                    }));
                if on {
                    state.toast(
                        s::member_blocked(&author.short),
                        ToastKind::Info,
                        Some(UiCommand::Chat(ChatCommand::SetMemberBlocked {
                            group: g,
                            member,
                            on: false,
                        })),
                        env.mono,
                    );
                }
            }
        }
    }
    if let Some(a) = chosen {
        let mut active = Active::default();
        apply(ui, state, env, snap, chat, all, a, &mut active);
    }
}
