//! Cover art painting and the generated cover for games without artwork (UI_DESIGN_SYSTEM
//! §7.2): a quiet wash of one of a few hues chosen by the name, with the game's initials set
//! large, tone on tone, in the lower left corner. The title is always beneath the cover, so
//! the generated cover never has to be read.

use egui::epaint::RectShape;
use egui::{
    Align2, Color32, CornerRadius, Mesh, Painter, Rect, Stroke, StrokeKind, TextureId, pos2,
};

use crate::theme::Theme;
use crate::theme::palette::{self, fade, mix};
use crate::theme::tokens::{alpha, stroke};
use crate::theme::typography::Type;

/// A stable 64-bit hash of a name (FNV-1a), the placeholder seed.
pub fn seed(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Words that carry no initial when a name has others ("Vale of Cinders" → "VC").
const SMALL_WORDS: [&str; 8] = ["a", "an", "and", "of", "the", "in", "on", "to"];

/// Up to two initials: first letters of the first two words that carry meaning
/// ("Hollow Knight" → "HK"); small words count only when nothing else does.
pub fn initials(name: &str) -> String {
    let words = || name.split(|c: char| c.is_whitespace() || c == ':' || c == '-');
    let pick = |skip_small: bool| -> String {
        words()
            .filter(|w| !(skip_small && SMALL_WORDS.contains(&w.to_lowercase().as_str())))
            .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
            .take(2)
            .flat_map(char::to_uppercase)
            .collect()
    };
    let mut out = pick(true);
    if out.is_empty() {
        out = pick(false);
    }
    if out.is_empty() {
        out.push('?');
    }
    out
}

/// The two ends of a generated cover's wash for `name` (also the ambient tint of a game
/// page without artwork).
pub fn placeholder_colors(name: &str, theme: &Theme) -> (Color32, Color32) {
    let c = palette::placeholder(seed(name), theme.palette.kind);
    (c.top, c.bottom)
}

/// A diagonal two-colour gradient in `rect` (top-left `a` → bottom-right `b`).
pub fn paint_gradient(painter: &Painter, rect: Rect, a: Color32, b: Color32) {
    let mut mesh = Mesh::default();
    let mid = mix(a, b, 0.5);
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), mid);
    mesh.colored_vertex(rect.right_bottom(), b);
    mesh.colored_vertex(rect.left_bottom(), mid);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// A vertical gradient from `top` to `bottom` colours.
pub fn paint_vertical_gradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// A vertical wash from `top` to `bottom` with rounded corners, as one mesh: a fan over the
/// rounded outline whose vertices take the colour of their height (a colour that is linear
/// in position is reproduced exactly by any triangulation), and a feather one pixel wide
/// around it for the edge. One shape, so the wash stays even while it fades in, and has no
/// seams between pixels. Generated covers and the game page's hero are painted with it.
pub fn paint_rounded_wash(
    painter: &Painter,
    rect: Rect,
    corner: CornerRadius,
    top: Color32,
    bottom: Color32,
) {
    /// Segments per corner.
    const ARC: usize = 8;
    let r = f32::from(corner.nw)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);
    let feather = 1.0 / painter.ctx().pixels_per_point();
    let at = |y: f32| mix(top, bottom, (y - rect.top()) / rect.height().max(1.0));
    // The outline clockwise from the top-left corner: (point, outward normal).
    let corners = [
        (pos2(rect.left() + r, rect.top() + r), 180.0_f32),
        (pos2(rect.right() - r, rect.top() + r), 270.0),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
    ];
    let mut mesh = Mesh::default();
    let centre = rect.center();
    mesh.colored_vertex(centre, at(centre.y));
    let mut ring = 0_u32;
    for (c, start) in corners {
        for i in 0..=ARC {
            let a = (start + 90.0 * i as f32 / ARC as f32).to_radians();
            let n = egui::vec2(a.cos(), a.sin());
            let edge = c + n * r;
            let inner = edge - n * (feather / 2.0);
            let outer = edge + n * (feather / 2.0);
            mesh.colored_vertex(inner, at(inner.y));
            mesh.colored_vertex(outer, Color32::TRANSPARENT);
            ring += 1;
        }
    }
    for i in 0..ring {
        let j = (i + 1) % ring;
        let (inner_a, outer_a) = (1 + 2 * i, 2 + 2 * i);
        let (inner_b, outer_b) = (1 + 2 * j, 2 + 2 * j);
        mesh.add_triangle(0, inner_a, inner_b);
        mesh.add_triangle(inner_a, outer_a, outer_b);
        mesh.add_triangle(inner_a, outer_b, inner_b);
    }
    painter.add(mesh);
}

/// What to paint as the cover.
#[derive(Clone, Copy, Debug)]
pub enum CoverSource {
    /// A loaded texture; `fade` 0–1 for the fade-in.
    Texture(TextureId, f32),
    /// Texture pending: dominant colour fill.
    Pending(Color32),
    /// No artwork: generated cover.
    Placeholder,
}

/// Paint a cover into `rect` with rounded corners.
pub fn paint_cover(
    painter: &Painter,
    rect: Rect,
    corner: CornerRadius,
    source: CoverSource,
    name: &str,
    theme: &Theme,
) {
    let colors = palette::placeholder(seed(name), theme.palette.kind);
    match source {
        CoverSource::Texture(id, t) => {
            if t < 1.0 {
                painter.rect_filled(rect, corner, colors.bottom);
            }
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter
                .add(RectShape::filled(rect, corner, fade(Color32::WHITE, t)).with_texture(id, uv));
        }
        CoverSource::Pending(c) => {
            painter.rect_filled(rect, corner, c);
        }
        CoverSource::Placeholder => {
            paint_rounded_wash(painter, rect, corner, colors.top, colors.bottom);
            let text = initials(name);
            let w = rect.width();
            if w < 64.0 {
                // A thumbnail: one initial, centred.
                let first: String = text.chars().take(1).collect();
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    first,
                    egui::FontId::new((w * 0.46).max(8.0), Type::Display.font_id().family),
                    colors.ink,
                );
            } else {
                // Set large in the lower left corner, like the spine mark of a case.
                let size = (w * if text.chars().count() > 1 { 0.40 } else { 0.50 }).min(140.0);
                let inset = (w * 0.085).max(8.0);
                painter.text(
                    pos2(rect.left() + inset, rect.bottom() - inset * 0.55),
                    Align2::LEFT_BOTTOM,
                    text,
                    egui::FontId::new(size.round(), Type::Display.font_id().family),
                    colors.ink,
                );
            }
        }
    }
}

/// The edge of a cover: a hairline of `on_art` that keeps dark artwork apart from the page,
/// brighter while the card is pointed at (`hot` 0–1).
pub fn paint_edge(painter: &Painter, rect: Rect, corner: CornerRadius, hot: f32, theme: &Theme) {
    let p = &theme.palette;
    let color = if p.allows_translucency() {
        let base = if p.kind.is_dark() { p.on_art } else { p.scrim };
        fade(
            base,
            egui::lerp(alpha::COVER_EDGE..=alpha::COVER_EDGE_HOT, hot),
        )
    } else {
        p.border_strong
    };
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(stroke::BORDER, color),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_of_names() {
        assert_eq!(initials("Hollow Knight"), "HK");
        assert_eq!(initials("celeste"), "C");
        assert_eq!(initials("Disco Elysium: The Final Cut"), "DE");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("Vale of Cinders"), "VC");
        assert_eq!(initials("The Long Winter Road"), "LW");
        assert_eq!(initials("The Of"), "TO");
    }

    #[test]
    fn seed_is_stable() {
        assert_eq!(seed("Hollow Knight"), seed("Hollow Knight"));
        assert_ne!(seed("a"), seed("b"));
    }
}
