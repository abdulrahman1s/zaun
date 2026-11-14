//! The EasyPlay mark: the shield of the application icon with the play triangle cut out of
//! it (UI_DESIGN_SYSTEM §1, §6). Painted from the icon's own outline so the sidebar, the
//! window icon and the desktop entry show one shape.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

/// The shield outline of `assets/icons/app/easyplay.svg`, in its 512-unit space, with the
/// curves flattened.
fn shield_outline() -> Vec<(f32, f32)> {
    let mut pts = vec![(256.0, 92.0), (388.0, 140.0)];
    // Q 396 143 396 152
    quad(&mut pts, (388.0, 140.0), (396.0, 143.0), (396.0, 152.0));
    pts.push((396.0, 250.0));
    // C 396 340 330 398 262 424
    cubic(
        &mut pts,
        (396.0, 250.0),
        (396.0, 340.0),
        (330.0, 398.0),
        (262.0, 424.0),
    );
    quad(&mut pts, (262.0, 424.0), (256.0, 426.0), (250.0, 424.0));
    cubic(
        &mut pts,
        (250.0, 424.0),
        (182.0, 398.0),
        (116.0, 340.0),
        (116.0, 250.0),
    );
    pts.push((116.0, 152.0));
    quad(&mut pts, (116.0, 152.0), (116.0, 143.0), (124.0, 140.0));
    pts
}

/// The play triangle of the icon, flattened.
fn play_outline() -> Vec<(f32, f32)> {
    let mut pts = vec![(222.0, 190.0)];
    quad(&mut pts, (222.0, 190.0), (222.0, 176.0), (234.0, 183.0));
    pts.push((324.0, 239.0));
    quad(&mut pts, (324.0, 239.0), (336.0, 247.0), (324.0, 255.0));
    pts.push((234.0, 311.0));
    quad(&mut pts, (234.0, 311.0), (222.0, 318.0), (222.0, 304.0));
    pts
}

const STEPS: usize = 8;

fn quad(out: &mut Vec<(f32, f32)>, a: (f32, f32), c: (f32, f32), b: (f32, f32)) {
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        out.push((
            u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0,
            u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1,
        ));
    }
}

fn cubic(out: &mut Vec<(f32, f32)>, a: (f32, f32), c1: (f32, f32), c2: (f32, f32), b: (f32, f32)) {
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let u = 1.0 - t;
        out.push((
            u * u * u * a.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * b.0,
            u * u * u * a.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * b.1,
        ));
    }
}

/// The shield spans x 116–396 and y 92–426 of the icon: map that box into `rect`, keeping
/// its proportions and centring it.
fn place(points: &[(f32, f32)], rect: Rect) -> Vec<Pos2> {
    let (x0, y0, w, h) = (116.0, 92.0, 280.0, 334.0);
    let s = (rect.width() / w).min(rect.height() / h);
    let ox = rect.center().x - w * s / 2.0;
    let oy = rect.center().y - h * s / 2.0;
    points
        .iter()
        .map(|(x, y)| pos2(ox + (x - x0) * s, oy + (y - y0) * s))
        .collect()
}

/// Paint the mark in `rect`: the shield in `fill`, the play triangle in `cutout` (the colour
/// of the surface behind it, or of text on the accent).
pub fn paint_mark(painter: &Painter, rect: Rect, fill: Color32, cutout: Color32) {
    painter.add(Shape::convex_polygon(
        place(&shield_outline(), rect),
        fill,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        place(&play_outline(), rect),
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
        for pts in [shield_outline(), play_outline()] {
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
        let rect = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(22.0, 22.0));
        for p in place(&shield_outline(), rect) {
            assert!(rect.expand(0.01).contains(p), "{p:?}");
        }
    }
}
