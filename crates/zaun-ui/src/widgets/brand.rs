//! The Zaun mark: a shield with the play triangle cut out of it (UI_DESIGN_SYSTEM §1,
//! §6). One shape in the top bar, on every poster and on the welcome page.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

/// The mark is drawn on a grid of this many units.
const GRID: f32 = 24.0;

// The two outlines of the mark's path, flattened (`assets/tools` keeps no copy: the path is
// `M11.3 2.26a2 2 0 0 1 1.4 0l6.2 2.3a1.7 1.7 0 0 1 1.1 1.6v5.2c0 4.7-2.9 8.3-7.3 10.3a1.7
// 1.7 0 0 1-1.4 0C6.9 19.66 4 16.06 4 11.36v-5.2a1.7 1.7 0 0 1 1.1-1.6l6.2-2.3Z` and
// `M10.1 8.5a.6.6 0 0 1 .9-.52l5.1 3a.6.6 0 0 1 0 1.04l-5.1 3a.6.6 0 0 1-.9-.52v-6Z`).
const SHIELD: [(f32, f32); 52] = [
    (11.300, 2.260),
    (11.528, 2.190),
    (11.762, 2.148),
    (12.000, 2.133),
    (12.238, 2.148),
    (12.472, 2.190),
    (12.700, 2.260),
    (18.900, 4.560),
    (19.208, 4.713),
    (19.478, 4.925),
    (19.701, 5.188),
    (19.866, 5.489),
    (19.967, 5.818),
    (20.000, 6.160),
    (20.000, 11.360),
    (19.940, 12.512),
    (19.765, 13.616),
    (19.478, 14.671),
    (19.085, 15.675),
    (18.591, 16.626),
    (18.000, 17.523),
    (17.317, 18.363),
    (16.548, 19.145),
    (15.697, 19.868),
    (14.769, 20.529),
    (13.768, 21.127),
    (12.700, 21.660),
    (12.475, 21.743),
    (12.240, 21.794),
    (12.000, 21.811),
    (11.760, 21.794),
    (11.525, 21.743),
    (11.300, 21.660),
    (10.232, 21.127),
    (9.231, 20.529),
    (8.303, 19.868),
    (7.452, 19.145),
    (6.683, 18.363),
    (6.000, 17.523),
    (5.409, 16.626),
    (4.915, 15.675),
    (4.522, 14.671),
    (4.235, 13.616),
    (4.060, 12.512),
    (4.000, 11.360),
    (4.000, 6.160),
    (4.033, 5.818),
    (4.134, 5.489),
    (4.299, 5.188),
    (4.522, 4.925),
    (4.792, 4.713),
    (5.100, 4.560),
];
const PLAY: [(f32, f32); 19] = [
    (10.100, 8.500),
    (10.152, 8.256),
    (10.298, 8.054),
    (10.514, 7.929),
    (10.763, 7.903),
    (11.000, 7.980),
    (16.100, 10.980),
    (16.260, 11.114),
    (16.364, 11.295),
    (16.401, 11.500),
    (16.364, 11.705),
    (16.260, 11.886),
    (16.100, 12.020),
    (11.000, 15.020),
    (10.763, 15.097),
    (10.514, 15.071),
    (10.298, 14.946),
    (10.152, 14.744),
    (10.100, 14.500),
];

/// Map the mark's grid into `rect` (a square; a wider or taller box centres it).
fn place(points: &[(f32, f32)], rect: Rect) -> Vec<Pos2> {
    let s = rect.width().min(rect.height()) / GRID;
    let ox = rect.center().x - GRID * s / 2.0;
    let oy = rect.center().y - GRID * s / 2.0;
    points
        .iter()
        .map(|(x, y)| pos2(ox + x * s, oy + y * s))
        .collect()
}

/// Paint the mark in `rect`: the shield in `fill`, the play triangle in `cutout` (the colour
/// of what is behind the mark, so the triangle reads as cut out of it).
pub fn paint_mark(painter: &Painter, rect: Rect, fill: Color32, cutout: Color32) {
    painter.add(Shape::convex_polygon(
        place(&SHIELD, rect),
        fill,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        place(&PLAY, rect),
        cutout,
        Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A convex polygon is what the tessellator fills correctly: every turn of both
    /// outlines goes the same way.
    #[test]
    fn outlines_are_convex() {
        for pts in [&SHIELD[..], &PLAY[..]] {
            let n = pts.len();
            let mut sign = 0.0_f32;
            for i in 0..n {
                let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
                let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
                if cross.abs() < 1e-3 {
                    continue;
                }
                assert!(sign == 0.0 || sign.signum() == cross.signum(), "turn {i}");
                sign = cross;
            }
        }
    }

    #[test]
    fn the_mark_stays_inside_its_box() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(20.0, 20.0));
        for p in place(&SHIELD, rect) {
            assert!(rect.expand(0.01).contains(p), "{p:?}");
        }
        // The shield spans the grid's x 4–20 and reaches from just under its top to just
        // above its bottom.
        let xs: Vec<f32> = SHIELD.iter().map(|p| p.0).collect();
        assert!((xs.iter().copied().fold(f32::MAX, f32::min) - 4.0).abs() < 0.01);
        assert!((xs.iter().copied().fold(f32::MIN, f32::max) - 20.0).abs() < 0.01);
    }
}
