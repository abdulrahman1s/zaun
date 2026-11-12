//! Skeletons: `bg.raised` blocks with `r.sm` shaped like the pending content, with a slow
//! shimmer (none under reduced motion) (§7.6, USER_EXPERIENCE §10).

use egui::{CornerRadius, Rect, Ui};

use crate::theme;
use crate::theme::palette::mix;
use crate::theme::tokens::{radius, timing};

/// Paint a skeleton block in `rect`. `corner` defaults to `r.sm`.
pub fn block(ui: &Ui, rect: Rect, corner: Option<CornerRadius>) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let corner = corner.unwrap_or(CornerRadius::same(radius::SM));
    let base = p.bg_raised;
    let fill = if theme.motion.shimmer() {
        let t = ui.input(|i| i.time) as f32;
        let phase = (t / timing::SHIMMER_PERIOD * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        ui.ctx().request_repaint();
        mix(base, p.text_primary, 0.02 + 0.03 * phase)
    } else {
        mix(base, p.text_primary, 0.03)
    };
    ui.painter().rect_filled(rect, corner, fill);
}

/// A skeleton line of text: a short rounded bar.
pub fn line(ui: &Ui, left_top: egui::Pos2, width: f32, height: f32) {
    block(
        ui,
        Rect::from_min_size(left_top, egui::vec2(width, height)),
        Some(CornerRadius::same(radius::XS)),
    );
}
