//! Avatars as the window draws them (`screens/chat/avatar.rs`, notes C and D): the
//! picture when there is one, else two initials in the display face on the person's paper;
//! a circle for people, a rounded square for groups; the status dot, the speaking ring and
//! the ringing call's ping ring around it.
use egui::epaint::RectShape;
use egui::{Align2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};
use zaun_core::chat::overlay::snapshot::{Person, Status};
use zaun_ui::theme::palette::{self, Paper};
use zaun_ui::theme::typography::display_font;

use crate::look::Look;
use crate::pictures::Pictures;

/// The initials' size against the diameter, and a group's corner against its side.
const INITIALS: f32 = 0.42;
const GROUP_RADIUS: f32 = 0.28;

fn paper_of(name: &str, paper: Option<u8>) -> Paper {
    match paper {
        Some(i) => palette::paper(usize::from(i)),
        None => {
            palette::paper((palette::fnv1a(&format!("av{name}")) as usize) % palette::PAPER_COUNT)
        }
    }
}

fn initials(name: &str, group: bool) -> String {
    let out: String = if group {
        zaun_ui::widgets::cover::initials(name)
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect()
    } else {
        name.split_whitespace()
            .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
            .take(2)
            .flat_map(char::to_uppercase)
            .collect()
    };
    if out.is_empty() { "?".to_owned() } else { out }
}

pub fn corner(rect: Rect, group: bool) -> CornerRadius {
    if group {
        CornerRadius::same((rect.width() * GROUP_RADIUS).round().min(255.0) as u8)
    } else {
        CornerRadius::same(255)
    }
}

/// The avatar of `who` filling `rect` (square), faded to `alpha`.
pub fn avatar(
    painter: &Painter,
    rect: Rect,
    who: &Person,
    group: bool,
    pictures: &Pictures,
    look: &Look,
    alpha: f32,
) {
    let corner = corner(rect, group);
    let fade = |c: Color32| Look::alpha(c, alpha);
    if let Some(tex) = pictures.get(who.picture) {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.add(Shape::Rect(
            RectShape::filled(rect, corner, fade(Color32::WHITE)).with_texture(tex.id(), uv),
        ));
    } else {
        let Paper { paper, ink } = paper_of(&who.name, who.paper);
        painter.rect_filled(rect, corner, fade(paper));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            initials(&who.name, group),
            display_font((rect.width() * INITIALS).round()),
            fade(ink),
        );
    }
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(1.0, fade(look.edge)),
        StrokeKind::Inside,
    );
}

/// The status dot at the avatar's bottom end, ringed in `ring` (the plane under it).
pub fn status(
    painter: &Painter,
    rect: Rect,
    status: Status,
    look: &Look,
    ring: Color32,
    rtl: bool,
) {
    let r = 6.0;
    let x = if rtl {
        rect.left() + 4.0
    } else {
        rect.right() - 4.0
    };
    let c = pos2(x, rect.bottom() - 4.0);
    painter.circle_filled(c, r + 2.5, ring);
    match status {
        Status::Online => {
            painter.circle_filled(c, r, look.amark);
        }
        Status::Away => {
            painter.circle_filled(c, r, look.amber);
            painter.circle_filled(c, 3.0, ring);
        }
        Status::Busy => {
            painter.circle_filled(c, r, look.coral);
            painter.rect_filled(
                Rect::from_center_size(c, egui::vec2(6.0, 2.0)),
                CornerRadius::same(1),
                ring,
            );
        }
        Status::Offline => {
            painter.circle_filled(c, r, look.t4);
            painter.circle_filled(c, 3.0, ring);
        }
    }
}

/// The speaking ring (notes D): a gap of `gap` in the plane's colour, then a ring 2 + 2 ×
/// level wide in the accent mark, half to fully strong with the level (0..=1).
pub fn speaking(painter: &Painter, rect: Rect, level: f32, look: &Look, alpha: f32) {
    let level = level.clamp(0.0, 1.0);
    let c = rect.center();
    // The gap in the plane's colour, so the ring never touches the picture.
    painter.circle_stroke(
        c,
        rect.width() / 2.0 + 1.0,
        Stroke::new(2.0, Look::alpha(look.bg, alpha)),
    );
    let r = rect.width() / 2.0 + 2.0;
    let w = 2.0 + 2.0 * level;
    let colour = Look::alpha(look.amark, (0.5 + 0.5 * level) * alpha);
    painter.circle_stroke(c, r + w / 2.0, Stroke::new(w, colour));
}

/// The ringing call's ping (`@keyframes ping`, 1.5 s): a 2-point ring 4 out from the
/// avatar, growing to 1.45× and fading; `t` is the phase 0..1 (`None` = reduced motion: a
/// still ring).
pub fn ping(painter: &Painter, center: Pos2, radius: f32, t: Option<f32>, look: &Look) {
    let r = radius + 4.0;
    painter.circle_stroke(center, r, Stroke::new(2.0, look.amark));
    if let Some(t) = t {
        let e = crate::motion::ease_out(t);
        let grow = r * (1.0 + 0.45 * e);
        painter.circle_stroke(
            center,
            grow,
            Stroke::new(2.0, Look::alpha(look.amark, 0.9 * (1.0 - t))),
        );
    }
}
