//! Loading blocks (CSS `.sk`): cover-shaped blocks (2:3, the cover's radius) in `o2` whose
//! opacity pulses 1 ↔ .45 over 1.4 s (`ease-in-out`), each column 90 ms after the one
//! before it (seven columns). Still under reduced motion.

use egui::{CornerRadius, Rect, Ui, vec2};

use super::cover;
use crate::theme;
use crate::theme::Density;
use crate::theme::motion::cubic_bezier;
use crate::theme::tokens::{alpha, layout, radius, timing, travel};

/// The pulse's opacity for column `k` at the time of `ui`'s frame (1 under reduced motion).
fn pulse(ui: &Ui, k: usize) -> f32 {
    let theme = theme::theme(ui.ctx());
    if !theme.motion.shimmer() {
        return 1.0;
    }
    ui.ctx().request_repaint();
    let delay = (k % travel::PULSE_COLUMNS) as f32 * travel::PULSE_STAGGER;
    let t = ui.input(|i| i.time) as f32 - delay;
    if t < 0.0 {
        return 1.0;
    }
    let u = (t / timing::SHIMMER_PERIOD).fract();
    let ease = |s: f32| cubic_bezier(0.42, 0.0, 0.58, 1.0, s);
    let low = alpha::PULSE_LOW;
    if u < 0.5 {
        1.0 - (1.0 - low) * ease(u * 2.0)
    } else {
        low + (1.0 - low) * ease((u - 0.5) * 2.0)
    }
}

/// A loading block in `rect` of the `k`-th column: `o2`, pulsing. `corner` defaults to
/// radius 6.
pub fn block_at(ui: &Ui, rect: Rect, corner: Option<CornerRadius>, k: usize) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let corner = corner.unwrap_or(CornerRadius::same(radius::SM));
    let a = pulse(ui, k);
    ui.painter()
        .rect_filled(rect, corner, p.o2.gamma_multiply(a));
}

/// A loading block in `rect`. `corner` defaults to radius 6.
pub fn block(ui: &Ui, rect: Rect, corner: Option<CornerRadius>) {
    block_at(ui, rect, corner, 0);
}

/// A loading line of text: a short rounded bar.
pub fn line(ui: &Ui, left_top: egui::Pos2, width: f32, height: f32) {
    block(
        ui,
        Rect::from_min_size(left_top, vec2(width, height)),
        Some(CornerRadius::same(radius::XS)),
    );
}

/// A cover that has not arrived, in the `k`-th column of the wall (2:3 from `rect`'s width,
/// the cover's radius).
pub fn cover(ui: &Ui, rect: Rect, k: usize) {
    let r = Rect::from_min_size(
        rect.min,
        vec2(rect.width(), rect.width() * layout::COVER_ASPECT),
    );
    block_at(ui, r, Some(cover::corner(r)), k);
}

/// A card that has not arrived: its cover block only (the wall writes nothing under its
/// covers). `density` is no longer used.
pub fn card(ui: &Ui, rect: Rect, density: Density) {
    let _ = density;
    cover(ui, rect, 0);
}
