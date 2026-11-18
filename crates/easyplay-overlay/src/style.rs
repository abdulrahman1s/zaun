//! egui's own style for the overlay: the app's fonts and type scale, and the dark visuals
//! tinted with the user's accent (the text cursor, the selection).
use easyplay_ui::theme::typography;
use egui::{Context, Stroke, Visuals};

use crate::look::Look;

pub fn fonts(ctx: &Context) {
    ctx.set_fonts(typography::font_definitions());
    ctx.all_styles_mut(|s| s.text_styles = typography::text_styles());
}

pub fn apply(ctx: &Context, look: &Look) {
    let mut v = Visuals::dark();
    v.selection.bg_fill = look.tint;
    v.selection.stroke = Stroke::new(1.0, look.atext);
    v.text_cursor.stroke = Stroke::new(1.5, look.amark);
    v.override_text_color = Some(look.t1);
    v.window_fill = look.bg;
    v.panel_fill = egui::Color32::TRANSPARENT;
    ctx.set_visuals(v);
}
