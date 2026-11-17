//! Tooltips (CSS `.tip`, notes §5 "Tooltip"): `tip_bg` with `tip_fg` text, radius 6, padded
//! 8 × 4, 12 / 500 on a 16 line, at most 300 wide; the shortcut, if any, after an 8 gap at
//! 65 % opacity. Shown 500 ms after the pointer settles (the style's `tooltip_delay`), at
//! once on keyboard focus; 7 below its target and centred on it (above when there is no
//! room); it fades in over 100 ms.
//!
//! Widgets that call `on_hover_text(tooltip::rich(..))` themselves get egui's popup frame
//! instead; use [`attach`], [`tooltip`], [`tooltip_at_pointer`], [`disabled`] or
//! [`at_rect`] for the design's look.

use egui::{
    CornerRadius, Frame, Id, Margin, Rect, RectAlign, Response, RichText, Sense, Tooltip, Ui, pos2,
    vec2,
};

use crate::a11y;
use crate::theme::tokens::{alpha, layout, overlay, radius};
use crate::theme::{self, Dur, Type};

/// `text` as tooltip text for egui's own tooltip frame (the menu plate): 12 / 500 in `t1`.
pub fn rich(ctx: &egui::Context, text: impl Into<String>) -> RichText {
    Type::Label.rich(text).color(theme::theme(ctx).palette.t1)
}

/// "Ctrl+N" as the design writes it in a tooltip: "Ctrl N".
fn spaced(s: &str) -> String {
    if s.len() <= 1 {
        s.to_owned()
    } else {
        s.split('+')
            .filter(|k| !k.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The tooltip's plate and text, in a popup whose frame is `Frame::NONE`.
fn paint(ui: &mut Ui, text: &str, shortcut: Option<&str>) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let a = theme::motion::appear(
        ui.ctx(),
        ui.layer_id().id.with("tip.in"),
        theme.motion.fade(Dur::Tip),
    );
    ui.multiply_opacity(a);
    let [px, py] = overlay::TIP_PAD;
    Frame::NONE
        .fill(p.tip_bg)
        .corner_radius(CornerRadius::same(radius::SM))
        .inner_margin(Margin::symmetric(px as i8, py as i8))
        .show(ui, |ui| {
            let kbd = shortcut.map(|s| Type::Label.line(ui, &spaced(s), p.tip_fg));
            let kbd_w = kbd
                .as_ref()
                .map_or(0.0, |g| g.size().x + overlay::TIP_KBD_GAP);
            let w = (layout::TIP_MAX_W - px * 2.0 - kbd_w).max(1.0);
            let g = Type::Label.galley(ui, text, p.tip_fg, w, usize::MAX);
            let size = vec2(
                g.size().x + kbd_w,
                g.size().y.max(Type::Label.line_height()),
            );
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let gw = g.size().x;
            ui.painter().galley(rect.min, g, p.tip_fg);
            if let Some(k) = kbd {
                let mut painter = ui.painter().clone();
                painter.multiply_opacity(alpha::TIP_KBD);
                painter.galley(
                    pos2(rect.left() + gw + overlay::TIP_KBD_GAP, rect.top()),
                    k,
                    p.tip_fg,
                );
            }
        });
}

/// Show the tooltip of `resp`: after the delay under the pointer, at once while it has
/// keyboard focus.
fn show(resp: &Response, text: &str, shortcut: Option<&str>, at_pointer: bool) {
    let keyboard = resp.has_focus() && !resp.hovered() && super::focus_visible(&resp.ctx);
    let mut tip = placed(if keyboard {
        Tooltip::for_widget(resp)
    } else if resp.enabled() {
        Tooltip::for_enabled(resp)
    } else {
        Tooltip::for_disabled(resp)
    });
    if at_pointer && !keyboard {
        tip = tip.at_pointer();
    }
    tip.show(|ui| paint(ui, text, shortcut));
}

/// The design's placement: 7 below the target and centred on it, above when there is no
/// room; no frame of egui's own.
fn placed(mut tip: Tooltip<'_>) -> Tooltip<'_> {
    tip.popup = tip
        .popup
        .frame(Frame::NONE)
        .gap(layout::TIP_GAP)
        .align(RectAlign::BOTTOM)
        .align_alternatives(&[
            RectAlign::TOP,
            RectAlign::BOTTOM_START,
            RectAlign::TOP_START,
        ])
        .width(layout::TIP_MAX_W);
    tip
}

/// The tooltip of a part of a widget, open while this is called: under `rect` (the bar of a
/// chart that the arrow keys chose). `owner` is the id of the widget the part belongs to.
pub fn at_rect(ui: &Ui, owner: Id, rect: Rect, text: &str) {
    placed(Tooltip::always_open(
        ui.ctx().clone(),
        ui.layer_id(),
        owner,
        rect,
    ))
    .show(|ui| paint(ui, text, None));
}

/// Attach `text` as a hover tooltip (and as the widget's accessible description).
pub fn attach(resp: Response, text: &str) -> Response {
    a11y::describe(&resp, text);
    show(&resp, text, None, false);
    resp
}

/// Attach `text` (and `shortcut`) as a tooltip; shown at once when the control has keyboard
/// focus.
pub fn tooltip(resp: Response, text: &str, shortcut: Option<&str>) -> Response {
    match shortcut {
        Some(s) => a11y::describe(&resp, &format!("{text} ({s})")),
        None => a11y::describe(&resp, text),
    }
    show(&resp, text, shortcut, false);
    resp
}

/// [`tooltip`] for a control as wide as its container (a row): beside the pointer, so that
/// it does not lie over the next row; under the control when it has keyboard focus.
pub fn tooltip_at_pointer(resp: Response, text: &str, shortcut: Option<&str>) -> Response {
    match shortcut {
        Some(s) => a11y::describe(&resp, &format!("{text} ({s})")),
        None => a11y::describe(&resp, text),
    }
    show(&resp, text, shortcut, true);
    resp
}

/// The tooltip of a disabled control: why it is unavailable ("Another game is running").
pub fn disabled(resp: Response, reason: &str) -> Response {
    a11y::describe(&resp, reason);
    show(&resp, reason, None, false);
    resp
}
