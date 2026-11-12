//! Tooltips: 500 ms after hover (the style's `tooltip_delay`), immediately on keyboard focus;
//! one short sentence plus the shortcut when one exists (USER_EXPERIENCE §8).

use egui::Response;

/// Attach `text` (and `shortcut`) as a tooltip; shown at once when the control has keyboard
/// focus.
pub fn tooltip(resp: Response, text: &str, shortcut: Option<&str>) -> Response {
    let tip = match shortcut {
        Some(s) => format!("{text} ({s})"),
        None => text.to_owned(),
    };
    if resp.has_focus() && !resp.hovered() {
        resp.show_tooltip_text(tip.clone());
    }
    resp.on_hover_text(tip)
}
