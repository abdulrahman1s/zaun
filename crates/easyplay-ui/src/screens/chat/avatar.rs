//! People are circles, groups are rounded squares, games are posters (chat.html section 8
//! AVATAR, STATUS DOT, COUNT PILL; styles `.av`, `.st`, `.cnt`). Owner: the list agent.
//! The signatures are shared by every part of the page: change a body freely, a signature
//! only with the lead.
//!
//! - An avatar is two initials in the display face at 42 % of its diameter on one of the
//!   ten paper-and-ink pairs (the posters' own), chosen by `hash("av" + name)` unless the
//!   person chose one; a 1 pt `edge` line inside its rim. A group is the same as a rounded
//!   square (radius 28 %). Its initials skip small words, as a cover's do.
//! - The status dot sits on the avatar's bottom corner (the trailing one: bottom left in a
//!   right-to-left interface), 12 pt, overhanging by 2, ringed 2.5 in the colour behind it.
//!   The shape says it too: online a solid accent-mark dot; away an amber ring (6 pt hole);
//!   do not disturb coral with a 6 × 2 bar; offline a `t4` ring. The large one (the 72 pt
//!   profile avatar) is 16 pt, 2 inside the corner, ringed 3, with an 8 pt hole or an 8 × 3
//!   bar.
//! - The count pill is 18 high, at least 18 wide, padded 5, on the accent fill, its count in
//!   11 pt semibold tabular figures ("99+" past 99).
//! - A picture (ADR-0038; chat-calls.html C) fills the circle or rounded square over the
//!   initials, which stay until it is decoded ([`pictured`]); it then fades in over 140
//!   linear (at once under reduced motion). One that cannot be decoded leaves the initials.

use easyplay_core::model::{
    ChatRef, ChatSnapshot, GroupMember, MessageAuthor, MyStatus, Picture, Presence,
};
use egui::text::LayoutJob;
use egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape as EShape, Stroke,
    StrokeKind, TextFormat, Ui, Vec2, emath::TSTransform, pos2, vec2,
};

use crate::theme::palette::{self, Paper};
use crate::theme::tokens::{poster as measure, radius, stroke};
use crate::theme::typography::display_font;
use crate::theme::{self, Theme};

/// The avatar's measures (chat.html `.av`, `.st`, `.cnt`).
mod m {
    /// Initials, as a share of the diameter.
    pub const INITIALS: f32 = 0.42;
    /// A group's corner, as a share of its side.
    pub const GROUP_RADIUS: f32 = 0.28;
    /// Status dot: diameter, ring, hole, bar height, overhang.
    pub const DOT: [f32; 5] = [12.0, 2.5, 6.0, 2.0, 2.0];
    /// The large status dot: diameter, ring, hole, bar height, inset.
    pub const DOT_LARGE: [f32; 5] = [16.0, 3.0, 8.0, 3.0, 2.0];
    /// The count pill: height (and least width), side padding.
    pub const PILL: [f32; 2] = [18.0, 5.0];
    /// The count's size.
    pub const PILL_TEXT: f32 = 11.0;
    /// The count's weight.
    pub const PILL_WEIGHT: u16 = 600;
}

/// A circle (a person) or a rounded square (a group, radius 28%).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A person.
    Person,
    /// A group.
    Group,
}

/// The paper-and-ink pair of `name`'s avatar: `paper` when chosen, else `hash("av" + name)`.
pub fn paper_of(name: &str, paper: Option<u8>) -> Paper {
    match paper {
        Some(i) => palette::paper(usize::from(i)),
        None => {
            palette::paper((palette::fnv1a(&format!("av{name}")) as usize) % palette::PAPER_COUNT)
        }
    }
}

/// The two initials of an avatar: a person's first two words, a group's first two words
/// that are not small words (as a cover's).
pub fn initials(name: &str, shape: Shape) -> String {
    // Only letters and digits: the display face has no emoji or symbols.
    let words: Vec<String> = name
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    let out: String = match shape {
        Shape::Group => crate::widgets::cover::initials(&words.join(" "))
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect(),
        Shape::Person => words
            .iter()
            .take(2)
            .filter_map(|w| w.chars().next())
            .flat_map(char::to_uppercase)
            .collect(),
    };
    if out.is_empty() { "?".to_owned() } else { out }
}

/// The corner of an avatar of `shape` filling `rect`.
pub fn corner(rect: Rect, shape: Shape) -> CornerRadius {
    match shape {
        Shape::Person => CornerRadius::same(radius::PILL),
        Shape::Group => {
            CornerRadius::same((rect.width() * m::GROUP_RADIUS).round().min(255.0) as u8)
        }
    }
}

/// The avatar filling `rect` (square): two initials in the display face at 42% of the
/// diameter on the paper-and-ink pair `paper`, or the one from `hash("av" + name)`.
pub fn avatar(painter: &Painter, rect: Rect, name: &str, paper: Option<u8>, shape: Shape) {
    let theme = theme::theme(painter.ctx());
    let Paper { paper, ink } = paper_of(name, paper);
    let corner = corner(rect, shape);
    painter.rect_filled(rect, corner, paper);
    let d = rect.width();
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials(name, shape),
        display_font((d * m::INITIALS).round()),
        ink,
    );
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(stroke::BORDER, theme.palette.edge),
        StrokeKind::Inside,
    );
}

/// [`avatar`] with `picture` over the initials: they are drawn until the picture is decoded,
/// then it fades in over 140 ms and the `edge` line is drawn over it.
pub fn pictured(
    painter: &Painter,
    rect: Rect,
    name: &str,
    paper: Option<u8>,
    shape: Shape,
    picture: Option<&Picture>,
) {
    let face = picture.and_then(|pic| super::pictures::avatar(painter.ctx(), pic));
    let Some((brush, since)) = face else {
        avatar(painter, rect, name, paper, shape);
        return;
    };
    let theme = theme::theme(painter.ctx());
    let t = if theme.motion.reduced {
        1.0
    } else {
        let now = painter.ctx().input(|i| i.time);
        ((now - since) as f32 / theme::Dur::Fast.secs()).clamp(0.0, 1.0)
    };
    if t < 1.0 {
        avatar(painter, rect, name, paper, shape);
        painter.ctx().request_repaint();
    }
    let corner = corner(rect, shape);
    let mut fill = egui::epaint::RectShape::filled(rect, corner, Color32::WHITE.gamma_multiply(t));
    fill.brush = Some(brush);
    painter.add(fill);
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(stroke::BORDER, theme.palette.edge),
        StrokeKind::Inside,
    );
}

/// The user's own picture.
pub fn mine(snap: &ChatSnapshot) -> Option<&Picture> {
    snap.profile.as_ref()?.picture.as_ref()
}

/// A friend's picture.
pub fn of_friend(snap: &ChatSnapshot, friend: i64) -> Option<&Picture> {
    snap.friend(friend)?.picture.as_ref()
}

/// The picture of a conversation: the friend's, or the group's.
pub fn of_chat(snap: &ChatSnapshot, chat: ChatRef) -> Option<&Picture> {
    match chat {
        ChatRef::Direct(f) => of_friend(snap, f),
        ChatRef::Group(g) => snap.group(g)?.picture.as_ref(),
    }
}

/// A group member's picture: yours, or your friend's as their own profile has it (it is
/// newer than the member list's copy), or the member's.
pub fn of_member<'a>(snap: &'a ChatSnapshot, mem: &'a GroupMember) -> Option<&'a Picture> {
    if mem.me {
        return mine(snap);
    }
    mem.friend
        .and_then(|f| of_friend(snap, f))
        .or(mem.picture.as_ref())
}

/// The picture of a message's author in `chat`.
pub fn of_author<'a>(
    snap: &'a ChatSnapshot,
    chat: ChatRef,
    author: &MessageAuthor,
) -> Option<&'a Picture> {
    match (author, chat) {
        (MessageAuthor::Me, _) => mine(snap),
        (MessageAuthor::Friend, ChatRef::Direct(f)) => of_friend(snap, f),
        (MessageAuthor::Friend, ChatRef::Group(_)) => None,
        (MessageAuthor::Member(name), ChatRef::Group(g)) => {
            let mem = snap
                .group(g)?
                .member_list
                .iter()
                .find(|m| m.name == *name)?;
            of_member(snap, mem)
        }
        (MessageAuthor::Member(_), ChatRef::Direct(_)) => None,
    }
}

/// The picture of the friend called `name` (where only a name is at hand, as in a call).
pub fn of_name<'a>(snap: &'a ChatSnapshot, name: &str) -> Option<&'a Picture> {
    snap.friends
        .iter()
        .find(|f| f.name == name)?
        .picture
        .as_ref()
}

/// What the user's own status looks like as a dot: invisible is offline.
pub fn my_presence(status: MyStatus) -> Presence {
    match status {
        MyStatus::Online => Presence::Online,
        MyStatus::Away => Presence::Away,
        MyStatus::Busy => Presence::Busy,
        MyStatus::Invisible => Presence::Offline,
    }
}

/// The rect of the status dot of the avatar at `avatar`.
pub fn status_rect(avatar: Rect, large: bool) -> Rect {
    let [d, _, _, _, off] = if large { m::DOT_LARGE } else { m::DOT };
    // The small dot overhangs the avatar; the large one sits inside its corner.
    let off = if large { -off } else { off };
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let x = if rtl {
        avatar.left() - off
    } else {
        avatar.right() + off - d
    };
    Rect::from_min_size(pos2(x, avatar.bottom() + off - d), vec2(d, d))
}

/// The status dot at the avatar's bottom right, ringed 2.5 in `ring` (the colour behind
/// it); 12pt, or 16pt with `large` (the 72pt profile avatar).
pub fn status(painter: &Painter, avatar: Rect, presence: Presence, ring: Color32, large: bool) {
    status_dot(
        painter,
        status_rect(avatar, large).center(),
        presence,
        ring,
        large,
    );
}

/// The status dot alone, centred on `c` (a menu's status items show it in their icon
/// column), ringed in `ring`.
pub fn status_dot(painter: &Painter, c: Pos2, presence: Presence, ring: Color32, large: bool) {
    let p = theme::theme(painter.ctx()).palette.clone();
    let [d, ring_w, hole, bar, _] = if large { m::DOT_LARGE } else { m::DOT };
    painter.circle_filled(c, d / 2.0 + ring_w, ring);
    match presence {
        Presence::Online => {
            painter.circle_filled(c, d / 2.0, p.accent_mark);
        }
        Presence::Away => {
            painter.circle_filled(c, d / 2.0, p.amber);
            painter.circle_filled(c, hole / 2.0, ring);
        }
        Presence::Busy => {
            painter.circle_filled(c, d / 2.0, p.coral);
            painter.rect_filled(
                Rect::from_center_size(c, vec2(hole, bar)),
                CornerRadius::same((bar / 2.0).round() as u8),
                ring,
            );
        }
        Presence::Offline => {
            painter.circle_filled(c, d / 2.0, p.t4);
            painter.circle_filled(c, hole / 2.0, ring);
        }
    }
}

/// The count as the pill writes it: "99+" past 99.
pub fn count_text(n: u32) -> String {
    if n > 99 {
        "99+".to_owned()
    } else {
        n.to_string()
    }
}

fn pill_font() -> FontId {
    FontId::new(
        m::PILL_TEXT,
        egui::FontFamily::Name(format!("tnum{}", m::PILL_WEIGHT).into()),
    )
}

/// The size of the count pill for `n`.
pub fn pill_size(ui: &Ui, n: u32) -> Vec2 {
    let g = ui
        .painter()
        .layout_no_wrap(count_text(n), pill_font(), Color32::PLACEHOLDER);
    let [h, pad] = m::PILL;
    vec2((g.size().x + pad * 2.0).max(h).ceil(), h)
}

/// Paint the count pill for `n` filling `rect` (from [`pill_size`]).
pub fn paint_pill(painter: &Painter, rect: Rect, n: u32) {
    let p = theme::theme(painter.ctx()).palette.clone();
    painter.rect_filled(rect, CornerRadius::same(radius::PILL), p.fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        count_text(n),
        pill_font(),
        p.on_fill,
    );
    if p.outlined() {
        painter.rect_stroke(
            rect,
            CornerRadius::same(radius::PILL),
            Stroke::new(stroke::BORDER, p.line2),
            StrokeKind::Inside,
        );
    }
}

/// The count pill (18 high, min 18 wide, 11 semibold, fill / on-fill) with its right edge
/// at `right` and vertical centre at `y`; returns its rect.
#[allow(dead_code)] // Shared API (the brief's `avatar::count_pill`); the list paints with `paint_pill`.
pub fn count_pill(ui: &Ui, right: f32, y: f32, n: u32) -> Rect {
    let size = pill_size(ui, n);
    let rect = Rect::from_min_size(pos2(right - size.x, y - size.y / 2.0), size);
    paint_pill(ui.painter(), rect, n);
    rect
}

/// A small poster that still shows its title (chat.html `.pn-i`, `.dt-seen`: Playing now
/// at 64, Also plays at 60): the game's paper with the title set as the poster sets it,
/// without the mark, where `widgets::cover` would show initials under 72.
pub fn small_poster(painter: &Painter, rect: Rect, title: &str, theme: &Theme) {
    let w = rect.width();
    let corner = crate::widgets::cover::corner(rect);
    let Paper { paper, ink } = palette::paper_for(title);
    painter.rect_filled(rect, corner, paper);
    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let set = crate::widgets::cover::poster(painter.ctx(), title);
    let left = rect.left() + w * measure::INSET_X;
    let mut y = rect.bottom() - w * (measure::INSET_BOTTOM + set.raise);
    for (text, size) in set.lines.iter().rev() {
        let size = size * w;
        y -= size * measure::LINE;
        poster_line(&clipped, pos2(left, y), text, size, ink);
    }
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(stroke::BORDER, theme.palette.edge),
        StrokeKind::Inside,
    );
}

/// One line of a small poster: its line box's top left at `pos`, set at `size`.
fn poster_line(painter: &Painter, pos: Pos2, text: &str, size: f32, ink: Color32) {
    if text.is_empty() || size <= 0.0 {
        return;
    }
    // Laid out at a whole size and scaled, so that small sizes keep their shapes.
    let at = size.ceil().max(8.0);
    let ordered = crate::i18n::bidi::visual_line(text, theme::typography::base());
    let mut job = LayoutJob::single_section(
        ordered.into_owned(),
        TextFormat {
            font_id: display_font(at),
            extra_letter_spacing: measure::TRACKING * at,
            line_height: Some(measure::LINE * at),
            color: ink,
            ..TextFormat::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    job.round_output_to_gui = false;
    let galley = painter.ctx().fonts_mut(|f| f.layout_job(job));
    let mut shape = EShape::galley(Pos2::ZERO, galley, ink);
    shape.transform(TSTransform::new(pos.to_vec2(), size / at));
    painter.add(shape);
}
