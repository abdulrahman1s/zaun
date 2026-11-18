//! The toasts' last line: "Shift+F1 to reply" in 11 t3 with the key in t2; without a key,
//! "Open the overlay to reply" (`chat.overlay_key` empty).
use easyplay_ui::theme::Type;
use egui::{Align2, Pos2, Ui, pos2};

use crate::look::Look;
use crate::model::Model;
use crate::paint::text;
use crate::words;

pub fn key_line(ui: &Ui, m: &Model, call: bool, at: Pos2, rtl: bool, a: f32) {
    let look = &m.look;
    let key = m.snap.look.key.trim();
    let quiet = Look::alpha(look.t3, a);
    if key.is_empty() {
        text::line(
            ui,
            Type::Micro,
            words::text("overlay.reply_hint_nokey"),
            quiet,
            pos2(at.x, at.y),
            260.0,
            rtl,
        );
        return;
    }
    let (before, after) = if call {
        &m.labels.answer_hint
    } else {
        &m.labels.reply_hint
    };
    let parts = [
        (before.as_str(), quiet),
        (key, Look::alpha(look.t2, a)),
        (after.as_str(), quiet),
    ];
    // Reading order; in a right-to-left line the first part sits at the right.
    let mut x = at.x;
    for (s, c) in &parts {
        if s.is_empty() {
            continue;
        }
        let g = text::galley(ui, Type::Micro, s, *c, 260.0, 1);
        let anchor = if rtl {
            Align2::RIGHT_TOP
        } else {
            Align2::LEFT_TOP
        };
        let r = text::put(ui, &g, pos2(x, at.y), anchor);
        x = if rtl { r.left() } else { r.right() };
    }
}
