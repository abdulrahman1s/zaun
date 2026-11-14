//! Tooltips: 500 ms after hover (the style's `tooltip_delay`), immediately on keyboard focus;
//! one short sentence in `caption`, plus the shortcut as keycaps when one exists
//! (USER_EXPERIENCE §8). The frame is the popup frame of `theme::apply`.

use egui::{Context, Response, RichText, Ui};

use super::key_hint::key_hint;
use crate::a11y;
use crate::theme::tokens::space;
use crate::theme::{self, Type};

/// `text` as tooltip text: `caption` in `text.primary`.
pub fn rich(ctx: &Context, text: impl Into<String>) -> RichText {
    Type::Caption
        .rich(text)
        .color(theme::theme(ctx).palette.text_primary)
}

/// Attach `text` as a hover tooltip (and as the widget's accessible description).
pub fn attach(resp: Response, text: &str) -> Response {
    let tip = rich(&resp.ctx, text);
    resp.on_hover_text(tip)
}

fn with_shortcut(ui: &mut Ui, text: &str, shortcut: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        ui.label(rich(ui.ctx(), text));
        key_hint(ui, shortcut);
    });
}

/// Attach `text` (and `shortcut`) as a tooltip; shown at once when the control has keyboard
/// focus.
pub fn tooltip(resp: Response, text: &str, shortcut: Option<&str>) -> Response {
    let keyboard = resp.has_focus() && !resp.hovered();
    match shortcut {
        Some(s) => {
            a11y::describe(&resp, &format!("{text} ({s})"));
            if keyboard {
                resp.show_tooltip_ui(|ui| with_shortcut(ui, text, s));
            }
            resp.on_hover_ui(|ui| with_shortcut(ui, text, s))
        }
        None => {
            if keyboard {
                resp.show_tooltip_text(rich(&resp.ctx, text));
            }
            attach(resp, text)
        }
    }
}

/// [`tooltip`] for a control as wide as its container (a row): beside the pointer, so that
/// it does not lie over the next row; under the control when it has keyboard focus.
pub fn tooltip_at_pointer(resp: Response, text: &str, shortcut: Option<&str>) -> Response {
    let keyboard = resp.has_focus() && !resp.hovered();
    match shortcut {
        Some(s) => {
            a11y::describe(&resp, &format!("{text} ({s})"));
            if keyboard {
                resp.show_tooltip_ui(|ui| with_shortcut(ui, text, s));
            }
            resp.on_hover_ui_at_pointer(|ui| with_shortcut(ui, text, s))
        }
        None => {
            a11y::describe(&resp, text);
            if keyboard {
                resp.show_tooltip_text(rich(&resp.ctx, text));
            }
            let tip = rich(&resp.ctx, text);
            resp.on_hover_text_at_pointer(tip)
        }
    }
}
