//! Cover art painting and the generated placeholder (UI_DESIGN_SYSTEM §7.2): `bg.raised` with
//! a soft diagonal gradient seeded by the name, initials in `display`.

use egui::epaint::RectShape;
use egui::{Align2, Color32, CornerRadius, Mesh, Painter, Rect, TextureId, pos2};

use crate::theme::Theme;
use crate::theme::palette::{fade, hsl, mix};
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

/// Up to two initials: first letters of the first two words ("Hollow Knight" → "HK").
pub fn initials(name: &str) -> String {
    let mut out: String = name
        .split(|c: char| c.is_whitespace() || c == ':' || c == '-')
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if out.is_empty() {
        out.push('?');
    }
    out
}

/// The two gradient stops of a placeholder, derived from the name and the theme's raised
/// surface: low-saturation hues so the generated covers stay quiet.
pub fn placeholder_colors(name: &str, theme: &Theme) -> (Color32, Color32) {
    let s = seed(name);
    let hue = (s % 360) as f32 / 360.0;
    let hue2 = hue + 0.08 + ((s >> 12) % 60) as f32 / 1000.0;
    let dark = theme.palette.kind.is_dark();
    let (l1, l2) = if dark { (0.30, 0.17) } else { (0.80, 0.68) };
    let base = theme.palette.bg_raised;
    (
        mix(base, hsl(hue, 0.38, l1), 0.85),
        mix(base, hsl(hue2, 0.30, l2), 0.85),
    )
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

/// What to paint as the cover.
#[derive(Clone, Copy, Debug)]
pub enum CoverSource {
    /// A loaded texture; `fade` 0–1 for the fade-in.
    Texture(TextureId, f32),
    /// Texture pending: dominant colour fill with the title visible.
    Pending(Color32),
    /// No artwork: generated placeholder.
    Placeholder,
}

/// Paint a cover into `rect` with rounded corners. `show_initials` draws the initials on the
/// generated placeholder (cards hide them when the title overlaps anyway at small sizes).
pub fn paint_cover(
    painter: &Painter,
    rect: Rect,
    corner: CornerRadius,
    source: CoverSource,
    name: &str,
    theme: &Theme,
) {
    let (a, b) = placeholder_colors(name, theme);
    match source {
        CoverSource::Texture(id, t) => {
            if t < 1.0 {
                painter.rect_filled(rect, corner, a);
            }
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter
                .add(RectShape::filled(rect, corner, fade(Color32::WHITE, t)).with_texture(id, uv));
        }
        CoverSource::Pending(c) => {
            painter.rect_filled(rect, corner, c);
        }
        CoverSource::Placeholder => {
            // Vertical a → b; the rounded ends are solid strips of the end colours so the
            // corners stay round and the seams invisible.
            let r = f32::from(corner.nw.max(corner.sw));
            let inner = rect.shrink2(egui::vec2(0.0, r));
            let top = Rect::from_min_max(rect.left_top(), pos2(rect.right(), inner.top()));
            let bottom = Rect::from_min_max(pos2(rect.left(), inner.bottom()), rect.right_bottom());
            painter.rect_filled(
                top,
                CornerRadius {
                    nw: corner.nw,
                    ne: corner.ne,
                    sw: 0,
                    se: 0,
                },
                a,
            );
            paint_vertical_gradient(painter, inner, a, b);
            painter.rect_filled(
                bottom,
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: corner.sw,
                    se: corner.se,
                },
                b,
            );
            let small = rect.width() < 64.0;
            let size = if small {
                (rect.width() * 0.42).max(8.0)
            } else {
                (rect.width() * 0.30).clamp(18.0, Type::Display.size() * 1.6)
            };
            let y = if small {
                rect.center().y
            } else {
                rect.top() + rect.height() * 0.36
            };
            painter.text(
                pos2(rect.center().x, y),
                Align2::CENTER_CENTER,
                initials(name),
                egui::FontId::new(size, Type::Display.font_id().family),
                fade(
                    theme.palette.text_primary,
                    if theme.palette.kind.is_dark() {
                        0.55
                    } else {
                        0.45
                    },
                ),
            );
        }
    }
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
    }

    #[test]
    fn seed_is_stable() {
        assert_eq!(seed("Hollow Knight"), seed("Hollow Knight"));
        assert_ne!(seed("a"), seed("b"));
    }
}
