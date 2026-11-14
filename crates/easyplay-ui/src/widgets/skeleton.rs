//! Skeletons: quiet blocks shaped like the pending content, with a slow shimmer (none under
//! reduced motion) (§7.6, USER_EXPERIENCE §10).

use egui::{CornerRadius, Rect, Ui, pos2, vec2};

use crate::theme;
use crate::theme::palette::mix;
use crate::theme::tokens::{layout, radius, timing};
use crate::theme::{Density, Type};

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
        mix(base, p.text_primary, 0.01 + 0.035 * phase)
    } else {
        mix(base, p.text_primary, 0.03)
    };
    ui.painter().rect_filled(rect, corner, fill);
}

/// A skeleton line of text: a short rounded bar.
pub fn line(ui: &Ui, left_top: egui::Pos2, width: f32, height: f32) {
    block(
        ui,
        Rect::from_min_size(left_top, vec2(width, height)),
        Some(CornerRadius::same(radius::XS)),
    );
}

/// A card that has not arrived: the cover, a title bar and a shorter status bar, in the
/// cell `rect` of the grid (the caption is omitted where the density has none).
pub fn card(ui: &Ui, rect: Rect, density: Density) {
    let cover = Rect::from_min_size(
        rect.min,
        vec2(rect.width(), rect.width() * layout::COVER_ASPECT),
    );
    block(ui, cover, Some(CornerRadius::same(radius::POP)));
    let y = cover.bottom() + layout::CAPTION_GAP + 4.0;
    line(ui, pos2(rect.left(), y), rect.width() * 0.72, 12.0);
    if density != Density::Compact {
        line(
            ui,
            pos2(rect.left(), y + Type::BodyStrong.line_height()),
            rect.width() * 0.44,
            10.0,
        );
    }
}
