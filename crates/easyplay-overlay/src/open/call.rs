//! The call at the foot of the start column (only in a call): a hairline above, padding
//! 12 · 14 · 14; "Voice in Friday Crew" 12 medium t3; the people as 28 avatars 8 apart with
//! their speaking rings; four 44 × 36 pill buttons spread across: mute, stop hearing, share
//! the game, leave. Muted and not hearing are inverted with a stroke through the icon.
use easyplay_core::chat::overlay::FromOverlay;
use easyplay_core::chat::overlay::snapshot::OverlayCall;
use easyplay_ui::theme::{Type, icons};
use egui::{Color32, Id, Rect, Stroke, Ui};

use super::button;
use super::layout::Row;
use crate::model::Model;
use crate::paint::{avatar, icon, text};
use crate::words;

pub const HEIGHT: f32 = 12.0 + 16.0 + 10.0 + 28.0 + 12.0 + 36.0 + 14.0;

pub fn draw(ui: &mut Ui, m: &mut Model, call: &OverlayCall, rect: Rect) {
    let look = m.look;
    let rtl = words::rtl();
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0, look.line));
    let inner = Row::new(rect.shrink2(egui::vec2(14.0, 0.0)), rtl);
    let label = &m.labels.call;
    let y = rect.top() + 12.0;
    text::line(
        ui,
        Type::Label,
        label,
        look.t3,
        egui::pos2(inner.x(0.0), y),
        inner.rect.width(),
        rtl,
    );
    let ay = y + 16.0 + 10.0;
    for (i, p) in call.people.iter().enumerate() {
        let av = inner.span(i as f32 * 36.0, 28.0, ay, 28.0);
        if p.speaking {
            avatar::speaking(ui.painter(), av, f32::from(p.level) / 100.0, &look, 1.0);
        }
        avatar::avatar(ui.painter(), av, &p.who, false, &m.pictures, &look, 1.0);
    }
    let by = ay + 28.0 + 12.0;
    let w = inner.rect.width();
    let step = (w - 44.0) / 3.0;
    let mut said = None;
    let slots = [
        (
            icons::MIC,
            call.muted,
            words::text(if call.muted {
                "chat.details.unmute"
            } else {
                "chat.details.mute_mic"
            }),
            Kind::Toggle,
        ),
        (
            icons::HEADPHONES,
            call.deafened,
            words::text(if call.deafened {
                "chat.details.undeafen"
            } else {
                "chat.details.deafen"
            }),
            Kind::Toggle,
        ),
        (
            icons::SHARE_GAME,
            call.sharing,
            words::text(if call.sharing {
                "chat.details.stop_share_tip"
            } else {
                "chat.details.share_tip"
            }),
            Kind::Share,
        ),
        (
            icons::PHONE_END,
            false,
            words::text("chat.details.leave_call"),
            Kind::Leave,
        ),
    ];
    for (i, (glyph, on, tip, kind)) in slots.into_iter().enumerate() {
        if kind == Kind::Share && !call.can_share && !call.sharing {
            continue;
        }
        let r = inner.span(i as f32 * step, 44.0, by, 36.0);
        let (idle, fg) = match (kind, on) {
            (Kind::Leave, _) => (look.coral, look.on_coral),
            (Kind::Toggle, true) => (look.t1, look.bg),
            (Kind::Share, true) => (look.o2, look.atext),
            (_, false) => (look.o2, look.t1),
        };
        let resp = button::plate(ui, Id::unique(("ovl-call", i)), r, 18, &look, idle, tip);
        icon::icon(ui.painter(), r.center(), glyph, 16.0, fg);
        if kind == Kind::Toggle && on {
            icon::slash(
                ui.painter(),
                r.center(),
                22.0,
                fg,
                Color32::TRANSPARENT,
                true,
            );
        }
        if resp.clicked() {
            if i == 2 && !call.sharing {
                // The desktop asks which window to share: that dialog must not sit under the
                // panel, which holds the keyboard.
                m.said.push(FromOverlay::Closed);
            }
            said = Some(match i {
                0 => FromOverlay::CallMute { on: !call.muted },
                1 => FromOverlay::CallDeafen { on: !call.deafened },
                2 => FromOverlay::Share { on: !call.sharing },
                _ => FromOverlay::EndCall,
            });
        }
    }
    m.said.extend(said);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Toggle,
    Share,
    Leave,
}
