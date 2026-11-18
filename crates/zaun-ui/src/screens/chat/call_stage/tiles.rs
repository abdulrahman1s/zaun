//! The stage's tiles (notes D, E): one poster per person on their paper colour, the speaking
//! ring from the voice level, the name chip with mute and stop-hearing marks, and a friend's
//! shared game. Every tile is a button that focuses it.

use egui::{
    Align2, Color32, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use zaun_core::model::{Call, ChatLive, ChatSnapshot, Picture};

use super::layout::{self, m};
use crate::screens::chat::avatar::{self, Shape};
use crate::screens::chat::paint;
use crate::strings::chat::calls as cs;
use crate::theme::palette::{ThemeKind, fade, mix};
use crate::theme::tokens::stroke;
use crate::theme::{self, Dur, Type, icons};
use crate::vm::chat_call::Tile;

/// A tile's corner.
const RADIUS: u8 = 10;
/// A friend's tile while ringing or reconnecting.
const DIM: f32 = 0.55;
/// The level at which someone counts as speaking (the service's gate).
const SPEAKING: u8 = 12;
/// A group member's ring: the service publishes who speaks, not how loud.
const GROUP_LEVEL: f32 = 0.6;

/// Where the call is, for the friend's tile and the status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Calling,
    Connecting,
    Reconnecting,
    Live,
}

/// The call's phase from its link and clock.
pub fn phase(call: &Call, live: &ChatLive) -> Phase {
    use zaun_core::model::CallLink as L;
    match (call.since, live.link) {
        (_, L::Interrupted) => Phase::Reconnecting,
        (_, L::Connecting) => Phase::Connecting,
        (None, _) => Phase::Calling,
        (Some(_), L::Ringing | L::Connected) => Phase::Live,
    }
}

/// One person on the stage.
pub struct Person<'a> {
    pub key: Tile,
    pub name: String,
    pub paper: Option<u8>,
    pub picture: Option<&'a Picture>,
    /// The voice level, 0–1.
    pub level: f32,
    pub speaking: bool,
    pub muted: bool,
    pub deafened: bool,
    /// A friend still being rung, connected or reconnected.
    pub waiting: Option<Phase>,
}

/// The others in the call, then the user.
pub fn people<'a>(snap: &'a ChatSnapshot, call: &Call) -> Vec<Person<'a>> {
    let live = &snap.live;
    let one = call.with.len() == 1;
    let ph = phase(call, live);
    let waiting = (ph != Phase::Live).then_some(ph);
    let mut out: Vec<Person<'a>> = call
        .with
        .iter()
        .map(|n| {
            let friend = snap.friends.iter().find(|f| f.name == *n);
            let speaking = call.speaking.contains(n);
            let level = if one {
                f32::from(live.levels.friend) / 100.0
            } else if speaking {
                GROUP_LEVEL
            } else {
                0.0
            };
            Person {
                key: Tile::Person(n.clone()),
                name: n.clone(),
                paper: friend.and_then(|f| f.paper),
                picture: friend.and_then(|f| f.picture.as_ref()),
                level,
                // Nobody is heard while the call rings, connects or reconnects.
                speaking: speaking && waiting.is_none(),
                muted: false,
                deafened: false,
                waiting,
            }
        })
        .collect();
    let me = snap.profile.as_ref();
    out.push(Person {
        key: Tile::Me,
        name: me.map(|p| p.name.clone()).unwrap_or_default(),
        paper: me.and_then(|p| p.paper),
        picture: avatar::mine(snap),
        level: f32::from(live.levels.me) / 100.0,
        speaking: !call.muted && live.levels.me >= SPEAKING,
        muted: call.muted,
        deafened: call.deafened,
        waiting: None,
    });
    out
}

/// A tile's size class.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Grid,
    Strip,
}

/// One person's tile in `rect`; `alpha` is the stage layer's opacity.
pub fn person(ui: &mut Ui, rect: Rect, who: &Person<'_>, size: Size, alpha: f32) -> Response {
    let th = theme::theme(ui.ctx());
    let p = &th.palette;
    let id = Id::unique(("chat.call.tile", &who.key));
    let resp = ui.interact(rect, id, Sense::click());
    crate::a11y::button(&resp, &spoken(who));
    let base = mix(
        p.bg,
        avatar::paper_of(&who.name, who.paper).paper,
        if p.kind == ThemeKind::Light {
            0.72
        } else {
            0.42
        },
    );
    let a = alpha
        * if who.waiting.is_some_and(|w| w != Phase::Connecting) {
            DIM
        } else {
            1.0
        };
    plate(ui, rect, &resp, base, a);
    let d = match size {
        Size::Grid => layout::avatar(rect.size()),
        Size::Strip => m::STRIP_AVATAR,
    };
    let av = Rect::from_center_size(rect.center(), vec2(d, d));
    ring(ui, av, who, size, base, a);
    let mut painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.set_opacity(a);
    avatar::pictured(
        &painter,
        av,
        &who.name,
        who.paper,
        Shape::Person,
        who.picture,
    );
    chip(ui, rect, who, size, a);
    resp
}

/// The tile's poster plate: brighter under the pointer, darker pressed, the focus ring.
fn plate(ui: &Ui, rect: Rect, resp: &Response, base: Color32, alpha: f32) {
    let th = theme::theme(ui.ctx());
    let hover = crate::widgets::hover_t(resp, &th);
    let fill = if resp.is_pointer_button_down_on() {
        mix(base, Color32::BLACK, 0.04)
    } else {
        mix(base, Color32::WHITE, 0.08 * hover)
    };
    let corner = CornerRadius::same(RADIUS);
    ui.painter().rect_filled(rect, corner, fade(fill, alpha));
    if th.palette.outlined() {
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(stroke::BORDER, fade(th.palette.line, alpha)),
            StrokeKind::Inside,
        );
    }
    if resp.has_focus() {
        crate::widgets::focus_ring(ui.painter(), rect, corner, &th);
    }
}

/// A friend's shared game as a tile: the live picture, or its stand-in until one comes.
pub fn stream(
    ui: &mut Ui,
    rect: Rect,
    title: &str,
    who: &str,
    frames: Option<&zaun_core::service::FrameSlot>,
    alpha: f32,
) -> Response {
    let resp = ui.interact(rect, Id::unique("chat.call.tile.stream"), Sense::click());
    crate::a11y::button(&resp, who);
    crate::screens::chat::stage_picture::picture(ui, rect, title, alpha, frames, None);
    let painter = ui.painter();
    let g = Type::Label.galley(ui, who, Color32::WHITE, rect.width() - 32.0, 1);
    let r = chip_rect(rect, g.size().x + 16.0, 24.0, 8.0);
    painter.rect_filled(r, CornerRadius::same(6), fade(Color32::BLACK, 0.6 * alpha));
    painter.galley(
        pos2(r.left() + 8.0, r.center().y - g.size().y / 2.0),
        g,
        Color32::WHITE,
    );
    if resp.has_focus() {
        let th = theme::theme(ui.ctx());
        crate::widgets::focus_ring(painter, rect, CornerRadius::same(RADIUS), &th);
    }
    resp
}

/// The speaking ring (a gap in the tile's colour, then accent by the level), or the ping
/// of a friend being rung.
fn ring(ui: &Ui, av: Rect, who: &Person<'_>, size: Size, tile: Color32, alpha: f32) {
    let th = theme::theme(ui.ctx());
    let painter = ui.painter();
    if who.waiting == Some(Phase::Calling) {
        return ping(ui, av, alpha);
    }
    let on = ui.ctx().animate_bool_with_time(
        Id::unique(("chat.call.speaking", &who.key)),
        who.speaking,
        th.motion.fade(Dur::Fast),
    );
    if on <= 0.0 {
        return;
    }
    let level = if th.motion.reduced {
        0.0
    } else {
        smooth(ui, who)
    };
    let (gap, base, grow) = match size {
        Size::Grid => (3.0, 2.0, 3.0),
        Size::Strip => (2.0, 2.0, 2.0),
    };
    let w = base + grow * level;
    let r = av.width() / 2.0;
    painter.circle_stroke(
        av.center(),
        r + gap / 2.0,
        Stroke::new(gap, fade(tile, alpha)),
    );
    painter.circle_stroke(
        av.center(),
        r + gap + w / 2.0,
        Stroke::new(
            w,
            fade(th.palette.accent_mark, (0.5 + 0.5 * level) * on * alpha),
        ),
    );
}

/// The level, smoothed: quick to rise (50 ms), slow to fall (300 ms).
fn smooth(ui: &Ui, who: &Person<'_>) -> f32 {
    let id = Id::unique(("chat.call.level", &who.key));
    let last = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let secs = if who.level > last { 0.05 } else { 0.3 };
    let v = ui
        .ctx()
        .animate_value_with_time(id.with("v"), who.level, secs);
    ui.data_mut(|d| d.insert_temp(id, v));
    v.clamp(0.0, 1.0)
}

/// The incoming card's ping on a friend being rung: 2 accent, growing 1 → 1.45 as it fades
/// every 1.5 s; still under reduced motion or while the window is behind another.
fn ping(ui: &Ui, av: Rect, alpha: f32) {
    let th = theme::theme(ui.ctx());
    let (scale, a) = if paint::moving(ui.ctx()) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
        let t = ((ui.input(|i| i.time) / 1.5).fract()) as f32;
        let e = theme::motion::ease_out(t);
        (1.0 + 0.45 * e, 0.9 * (1.0 - e))
    } else {
        (1.0, 0.9)
    };
    ui.painter().circle_stroke(
        av.center(),
        (av.width() / 2.0 + 4.0) * scale,
        Stroke::new(2.0, fade(th.palette.accent_mark, a * alpha)),
    );
}

/// The name chip at the tile's bottom start: black 60 %, the marks, then the name (or
/// "Connecting…" with a spinner, or "Reconnecting…").
fn chip(ui: &Ui, rect: Rect, who: &Person<'_>, size: Size, alpha: f32) {
    let (h, font, inset, glyph) = match size {
        Size::Grid => (24.0, Type::Label, 8.0, 13.0),
        Size::Strip => (18.0, Type::Micro, 4.0, 11.0),
    };
    let me = who.key == Tile::Me;
    let label = match who.waiting {
        Some(Phase::Connecting) => cs::CONNECTING(),
        Some(Phase::Reconnecting) => cs::RECONNECTING(),
        _ if me => crate::strings::chat::conversation::YOU(),
        _ => paint::first_name(&who.name),
    };
    let g = font.galley(
        ui,
        label,
        Color32::WHITE,
        (rect.width() - inset * 4.0).max(1.0),
        1,
    );
    let marks: Vec<theme::Icon> = [(who.muted, icons::MIC), (who.deafened, icons::HEADPHONES)]
        .into_iter()
        .filter_map(|(on, i)| on.then_some(i))
        .collect();
    let spin = who.waiting == Some(Phase::Connecting);
    let lead = (marks.len() as f32 + f32::from(u8::from(spin))) * (glyph + 4.0);
    let r = chip_rect(rect, lead + g.size().x + 16.0, h, inset);
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(r, CornerRadius::same(6), fade(Color32::BLACK, 0.6 * alpha));
    let white = fade(Color32::WHITE, alpha);
    let mut x = 8.0;
    let at = |x: f32, w: f32| if rtl { r.right() - x - w } else { r.left() + x };
    if spin {
        crate::widgets::progress::spinner(
            ui,
            pos2(at(x, glyph) + glyph / 2.0, r.center().y),
            5.5,
            white,
        );
        x += glyph + 4.0;
    }
    for icon in marks {
        let c = pos2(at(x, glyph) + glyph / 2.0, r.center().y);
        slashed(&painter, c, icon, glyph, white);
        x += glyph + 4.0;
    }
    painter.galley(
        pos2(at(x, g.size().x), r.center().y - g.size().y / 2.0),
        g,
        white,
    );
}

/// `icon` at `size` with a stroke through it (muted, not hearing).
pub fn slashed(
    painter: &egui::Painter,
    c: egui::Pos2,
    icon: theme::Icon,
    size: f32,
    color: Color32,
) {
    painter.text(
        c,
        Align2::CENTER_CENTER,
        icon.text(false),
        icons::font(size),
        color,
    );
    let d = vec2(size, -size) * 0.42;
    painter.line_segment([c - d, c + d], Stroke::new(1.5, color));
}

/// A chip of `w` × `h` at the tile's bottom start, `inset` in.
fn chip_rect(tile: Rect, w: f32, h: f32, inset: f32) -> Rect {
    let w = w.min(tile.width() - inset * 2.0).max(1.0);
    let r = Rect::from_min_size(
        pos2(tile.left() + inset, tile.bottom() - inset - h),
        vec2(w, h),
    );
    layout::mirror(r, tile, crate::i18n::dir() == crate::i18n::Dir::Rtl)
}

/// What a screen reader says for a tile: "Mira, speaking, muted".
fn spoken(who: &Person<'_>) -> String {
    let name = if who.key == Tile::Me {
        crate::strings::chat::conversation::YOU()
    } else {
        who.name.as_str()
    };
    let mut parts = vec![name];
    if who.speaking {
        parts.push(cs::TILE_SPEAKING());
    }
    if who.muted {
        parts.push(cs::TILE_MUTED());
    }
    if who.deafened {
        parts.push(cs::TILE_DEAFENED());
    }
    crate::strings::chat::spoken(&parts)
}
