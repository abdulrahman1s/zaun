//! The voice widget (`chat-calls.html` §G CLOSED): while the user is in a call, a column of
//! pills in the chosen corner, 24 from both edges, 4 apart. Each pill is 32 high on the
//! plate (the background at 78 % with a 1-point line): the avatar 24 (4 in), the first name
//! 13 medium (t2, t1 while speaking), a slashed microphone when muted, 12 at the end. At most
//! 8 pills, then "and N more". The user is last. Corners are physical: they never mirror.
use easyplay_core::chat::overlay::Corner;
use easyplay_core::chat::overlay::snapshot::OverlayCall;
use easyplay_ui::theme::{Type, icons};
use egui::{Align2, CornerRadius, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::look::Look;
use crate::model::Model;
use crate::paint::{avatar, icon, text};

const EDGE: f32 = 24.0;
const PILL_H: f32 = 32.0;
const GAP: f32 = 4.0;
const AVATAR: f32 = 24.0;

/// The column's height: one pill per person, and one for "and N more".
pub fn column(call: &OverlayCall) -> f32 {
    let count = call.people.len() + usize::from(call.more > 0);
    count as f32 * PILL_H + count.saturating_sub(1) as f32 * GAP
}

pub fn draw(ui: &Ui, m: &Model, call: &OverlayCall, alpha: f32) {
    let screen = ui.clip_rect();
    let look = &m.look;
    let right = matches!(m.snap.look.corner, Corner::TopRight | Corner::BottomRight);
    let bottom = matches!(m.snap.look.corner, Corner::BottomLeft | Corner::BottomRight);
    let column = column(call);
    let mut y = if bottom {
        screen.bottom() - EDGE - column
    } else {
        screen.top() + EDGE
    };
    let fade = |c| Look::alpha(c, alpha);
    let pill = |width: f32, y: f32| {
        let x = if right {
            screen.right() - EDGE - width
        } else {
            screen.left() + EDGE
        };
        let rect = Rect::from_min_size(pos2(x, y), vec2(width, PILL_H));
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(16), fade(look.plate));
        p.rect_stroke(
            rect,
            CornerRadius::same(16),
            Stroke::new(1.0, fade(look.line)),
            StrokeKind::Inside,
        );
        rect
    };
    // The call lists the user last (`build`); the names are made once per snapshot.
    let name = |i: usize| m.labels.people.get(i).map_or("", String::as_str);
    let galley = |i: usize, speaking: bool| {
        let colour = if speaking { look.t1 } else { look.t2 };
        text::galley(ui, Type::BodyStrong, name(i), fade(colour), 160.0, 1)
    };
    // The pills share one width, the widest (a CSS grid column).
    let width = call
        .people
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let w = galley(i, p.speaking).size().x;
            4.0 + AVATAR + 8.0 + w + if p.muted { 6.0 + 13.0 } else { 0.0 } + 12.0
        })
        .fold(0.0, f32::max);
    for (i, person) in call.people.iter().enumerate() {
        let g = &galley(i, person.speaking);
        let rect = pill(width, y);
        let av = Rect::from_min_size(
            pos2(rect.left() + 4.0, rect.top() + 4.0),
            vec2(AVATAR, AVATAR),
        );
        if person.speaking {
            avatar::speaking(
                ui.painter(),
                av,
                f32::from(person.level) / 100.0,
                look,
                alpha,
            );
        }
        avatar::avatar(
            ui.painter(),
            av,
            &person.who,
            false,
            &m.pictures,
            look,
            alpha,
        );
        let tx = av.right() + 8.0;
        text::put(ui, g, pos2(tx, rect.center().y), Align2::LEFT_CENTER);
        if person.muted {
            let c = pos2(tx + g.size().x + 6.0 + 6.5, rect.center().y);
            icon::icon(ui.painter(), c, icons::MIC, 14.0, fade(look.t3));
            icon::slash(
                ui.painter(),
                c,
                15.0,
                fade(look.t3),
                fade(look.plate),
                false,
            );
        }
        y += PILL_H + GAP;
    }
    if call.more > 0 {
        let g = text::galley(ui, Type::Caption, &m.labels.more, fade(look.t3), 200.0, 1);
        let rect = pill(12.0 + g.size().x + 12.0, y);
        text::put(ui, &g, rect.center(), Align2::CENTER_CENTER);
    }
}
