//! Colour roles for the dark, light and high-contrast themes and the accent alternatives
//! (UI_DESIGN_SYSTEM §2). The only place in the crate where colour values are written down.

use egui::Color32;

/// `#RRGGBB` as an opaque colour.
const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// `#RRGGBB` at `alpha` (0–255), unmultiplied.
fn hex_a(rgb: u32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha)
}

/// Fraction (0–1) to an 8-bit alpha.
fn alpha_u8(fraction: f32) -> u8 {
    (fraction.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Which palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeKind {
    /// Default.
    Dark,
    /// Light.
    Light,
    /// Pure black and white with strong outlines.
    HighContrast,
}

impl ThemeKind {
    /// Every theme.
    pub const ALL: [ThemeKind; 3] = [ThemeKind::Dark, ThemeKind::Light, ThemeKind::HighContrast];

    /// Whether egui should treat this as a dark theme.
    pub const fn is_dark(self) -> bool {
        !matches!(self, ThemeKind::Light)
    }
}

/// Accent alternatives (§2.4). An accent changes only the `accent.*` and `focus` roles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Accent {
    /// Teal (default).
    #[default]
    Tide,
    /// Orange.
    Ember,
    /// Indigo.
    Iris,
    /// Green.
    Moss,
}

impl Accent {
    /// Every accent.
    pub const ALL: [Accent; 4] = [Accent::Tide, Accent::Ember, Accent::Iris, Accent::Moss];

    /// The configuration value (`appearance.accent`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Accent::Tide => "tide",
            Accent::Ember => "ember",
            Accent::Iris => "iris",
            Accent::Moss => "moss",
        }
    }

    /// Parse [`Accent::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }

    /// (accent, hover, pressed, on-accent text, focus) for a dark background.
    fn dark(self) -> [u32; 5] {
        match self {
            Accent::Tide => [0x2DD4BF, 0x5EEAD4, 0x14B8A6, 0x06201C, 0x7DF0DF],
            Accent::Ember => [0xFF8A5B, 0xFFA680, 0xF0703F, 0x2A0E04, 0xFFB894],
            Accent::Iris => [0xA5B4FC, 0xC0CBFD, 0x8B9CF8, 0x10143A, 0xC7D2FE],
            Accent::Moss => [0xA3E635, 0xBEF264, 0x84CC16, 0x142305, 0xC5F27A],
        }
    }

    /// (accent, hover, pressed, on-accent text, focus) for a light background.
    fn light(self) -> [u32; 5] {
        match self {
            Accent::Tide => [0x0A7D6F, 0x0B8F7F, 0x086A5E, 0xFFFFFF, 0x0A7D6F],
            Accent::Ember => [0xC2410C, 0xD4500F, 0xA8380A, 0xFFFFFF, 0xC2410C],
            Accent::Iris => [0x4338CA, 0x5046E0, 0x3730A3, 0xFFFFFF, 0x4338CA],
            Accent::Moss => [0x4D7C0F, 0x5A8F12, 0x3F650C, 0xFFFFFF, 0x4D7C0F],
        }
    }
}

/// Every colour role of §2. Widgets read these through [`crate::theme::Theme`].
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    /// Which theme this is.
    pub kind: ThemeKind,
    /// Behind sheets, log viewer.
    pub bg_sunken: Color32,
    /// Window background.
    pub bg_base: Color32,
    /// Sidebar, cards, panels.
    pub bg_surface: Color32,
    /// Hovered surface, inputs, menus.
    pub bg_raised: Color32,
    /// Sheets, dialogs, popovers.
    pub bg_overlay: Color32,
    /// Outlines and separators.
    pub border_subtle: Color32,
    /// Inputs, focused card outline base.
    pub border_strong: Color32,
    /// Titles and body.
    pub text_primary: Color32,
    /// Supporting text.
    pub text_secondary: Color32,
    /// Metadata, placeholders.
    pub text_muted: Color32,
    /// Disabled labels.
    pub text_disabled: Color32,
    /// Primary action fill.
    pub accent: Color32,
    /// Accent, hovered.
    pub accent_hover: Color32,
    /// Accent, pressed.
    pub accent_pressed: Color32,
    /// Selected row background, chips.
    pub accent_subtle: Color32,
    /// Text and icons on accent fills.
    pub text_on_accent: Color32,
    /// Running state, success.
    pub success: Color32,
    /// Warnings.
    pub warning: Color32,
    /// Destruction, errors, tamper flag.
    pub danger: Color32,
    /// Information.
    pub info: Color32,
    /// Focus ring.
    pub focus: Color32,
    /// Behind modal layers.
    pub backdrop: Color32,
    /// Overlay shadow colour.
    pub shadow: Color32,
    /// Black scrim for badges on artwork and the card gradient end.
    pub scrim: Color32,
    /// White text used over artwork (independent of the theme).
    pub on_art: Color32,
    /// Secondary text over artwork.
    pub on_art_secondary: Color32,
    /// Tag hues `tag.0`…`tag.7`.
    pub tags: [Color32; 8],
}

impl Palette {
    /// The palette for `kind` with `accent` (ignored by high contrast, which has its own).
    pub fn new(kind: ThemeKind, accent: Accent) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(accent),
            ThemeKind::Light => Self::light(accent),
            ThemeKind::HighContrast => Self::high_contrast(),
        }
    }

    fn dark(accent: Accent) -> Self {
        let [a, ah, ap, on, focus] = accent.dark();
        Self {
            kind: ThemeKind::Dark,
            bg_sunken: hex(0x090B0F),
            bg_base: hex(0x0D0F14),
            bg_surface: hex(0x141821),
            bg_raised: hex(0x1B2030),
            bg_overlay: hex(0x232A3D),
            border_subtle: hex(0x232838),
            border_strong: hex(0x343C52),
            text_primary: hex(0xEEF0F6),
            text_secondary: hex(0xA9B0C3),
            text_muted: hex(0x8C95AE),
            text_disabled: hex(0x4B5266),
            accent: hex(a),
            accent_hover: hex(ah),
            accent_pressed: hex(ap),
            accent_subtle: hex_a(a, alpha_u8(0.12)),
            text_on_accent: hex(on),
            success: hex(0x4ADE80),
            warning: hex(0xFBBF24),
            danger: hex(0xF87171),
            info: hex(0x60A5FA),
            focus: hex(focus),
            backdrop: hex_a(0x000000, alpha_u8(0.55)),
            shadow: hex_a(0x000000, alpha_u8(0.45)),
            scrim: hex(0x000000),
            on_art: hex(0xFFFFFF),
            on_art_secondary: hex(0xD6DAE4),
            tags: [
                hex(0x7DD3FC),
                hex(0xC4B5FD),
                hex(0xF9A8D4),
                hex(0xFCD34D),
                hex(0xBEF264),
                hex(0x5EEAD4),
                hex(0xFDBA74),
                hex(0xCBD5E1),
            ],
        }
    }

    fn light(accent: Accent) -> Self {
        let [a, ah, ap, on, focus] = accent.light();
        Self {
            kind: ThemeKind::Light,
            bg_sunken: hex(0xECEEF2),
            bg_base: hex(0xF6F7F9),
            bg_surface: hex(0xFFFFFF),
            bg_raised: hex(0xFFFFFF),
            bg_overlay: hex(0xFFFFFF),
            border_subtle: hex(0xE3E6ED),
            border_strong: hex(0xC5CAD6),
            text_primary: hex(0x151821),
            text_secondary: hex(0x4A5165),
            text_muted: hex(0x656D82),
            text_disabled: hex(0xA8AEBD),
            accent: hex(a),
            accent_hover: hex(ah),
            accent_pressed: hex(ap),
            accent_subtle: hex_a(a, alpha_u8(0.10)),
            text_on_accent: hex(on),
            success: hex(0x15803D),
            warning: hex(0xB45309),
            danger: hex(0xB91C1C),
            info: hex(0x1D4ED8),
            focus: hex(focus),
            backdrop: hex_a(0x000000, alpha_u8(0.35)),
            shadow: hex_a(0x000000, alpha_u8(0.12)),
            scrim: hex(0x000000),
            on_art: hex(0xFFFFFF),
            on_art_secondary: hex(0xE4E7EE),
            tags: [
                hex(0x0369A1),
                hex(0x6D28D9),
                hex(0xBE185D),
                hex(0xB45309),
                hex(0x4D7C0F),
                hex(0x0F766E),
                hex(0xC2410C),
                hex(0x475569),
            ],
        }
    }

    fn high_contrast() -> Self {
        let white = hex(0xFFFFFF);
        let black = hex(0x000000);
        Self {
            kind: ThemeKind::HighContrast,
            bg_sunken: black,
            bg_base: black,
            bg_surface: black,
            bg_raised: hex(0x0A0A0A),
            bg_overlay: black,
            border_subtle: white,
            border_strong: white,
            text_primary: white,
            text_secondary: hex(0xF2F2F2),
            text_muted: hex(0xD9D9D9),
            text_disabled: hex(0x8C8C8C),
            accent: hex(0x5EEAD4),
            accent_hover: hex(0x99F6E4),
            accent_pressed: hex(0x2DD4BF),
            accent_subtle: black,
            text_on_accent: black,
            success: hex(0x4ADE80),
            warning: hex(0xFFE600),
            danger: hex(0xFF8080),
            info: hex(0x7CC4FF),
            focus: hex(0xFFE600),
            backdrop: black,
            shadow: Color32::TRANSPARENT,
            scrim: black,
            on_art: white,
            on_art_secondary: white,
            tags: [white; 8],
        }
    }

    /// The colour of tag hue `index` (wraps at 8).
    pub fn tag(&self, index: u8) -> Color32 {
        self.tags[usize::from(index) % self.tags.len()]
    }

    /// Whether translucency, shadows and ambient tint are allowed (not in high contrast).
    pub const fn allows_translucency(&self) -> bool {
        !matches!(self.kind, ThemeKind::HighContrast)
    }
}

/// `c` with its alpha multiplied by `fraction`.
pub fn fade(c: Color32, fraction: f32) -> Color32 {
    c.gamma_multiply(fraction.clamp(0.0, 1.0))
}

/// Mix `a` towards `b` by `t` (0–1) in gamma space.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// Lighten towards white by `amount` (0–1).
pub fn lighten(c: Color32, amount: f32) -> Color32 {
    mix(c, Color32::WHITE, amount)
}

/// Darken towards black by `amount` (0–1).
pub fn darken(c: Color32, amount: f32) -> Color32 {
    mix(c, Color32::BLACK, amount)
}

/// An opaque colour from hue (0–1), saturation (0–1) and lightness (0–1). Used for generated
/// placeholders; kept here so that colour arithmetic stays inside `theme`.
pub fn hsl(h: f32, s: f32, l: f32) -> Color32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(to(r), to(g), to(b))
}

/// A black colour at `fraction` opacity.
pub fn black(fraction: f32) -> Color32 {
    Color32::from_black_alpha(alpha_u8(fraction))
}

/// WCAG relative luminance of an opaque colour.
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Composite `fg` (possibly translucent, premultiplied) over opaque `bg`.
pub fn over(fg: Color32, bg: Color32) -> Color32 {
    let a = f32::from(fg.a()) / 255.0;
    let ch = |f: u8, b: u8| (f32::from(f) + f32::from(b) * (1.0 - a)).round().min(255.0) as u8;
    Color32::from_rgb(ch(fg.r(), bg.r()), ch(fg.g(), bg.g()), ch(fg.b(), bg.b()))
}

/// WCAG contrast ratio between two colours; translucent foregrounds are composited over `bg`.
pub fn contrast(fg: Color32, bg: Color32) -> f32 {
    let fg = over(fg, bg);
    let (a, b) = (luminance(fg), luminance(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (foreground role, background role, minimum ratio). Thresholds per ACCESSIBILITY §3;
    /// the full matrix is finalised in A11Y-002.
    fn pairs(p: &Palette) -> Vec<(&'static str, Color32, Color32, f32)> {
        let mut v = Vec::new();
        for (bg_name, bg) in [
            ("bg.base", p.bg_base),
            ("bg.surface", p.bg_surface),
            ("bg.raised", p.bg_raised),
            ("bg.overlay", p.bg_overlay),
        ] {
            v.push((bg_name, p.text_primary, bg, 4.5));
            v.push((bg_name, p.text_secondary, bg, 4.5));
            v.push((bg_name, p.text_muted, bg, 4.5));
            v.push((bg_name, p.danger, bg, 3.0));
            v.push((bg_name, p.warning, bg, 3.0));
            v.push((bg_name, p.success, bg, 3.0));
            v.push((bg_name, p.focus, bg, 3.0));
        }
        v.push(("accent", p.text_on_accent, p.accent, 4.5));
        v.push(("bg.base (accent)", p.accent, p.bg_base, 3.0));
        v
    }

    #[test]
    fn contrast_of_black_and_white() {
        let r = contrast(Color32::WHITE, Color32::BLACK);
        assert!((r - 21.0).abs() < 0.01, "{r}");
    }

    #[test]
    fn every_theme_and_accent_meets_contrast_minimums() {
        let mut failures = Vec::new();
        for kind in ThemeKind::ALL {
            for accent in Accent::ALL {
                let p = Palette::new(kind, accent);
                for (bg, fg, bgc, min) in pairs(&p) {
                    let r = contrast(fg, bgc);
                    if r < min {
                        failures.push(format!(
                            "{kind:?}/{accent:?}: {fg:?} on {bg} = {r:.2} < {min}"
                        ));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn card_gradient_keeps_white_titles_legible_over_any_art() {
        use crate::theme::tokens::alpha::CARD_GRADIENT;
        let p = Palette::new(ThemeKind::Dark, Accent::Tide);
        let scrim = black(CARD_GRADIENT);
        // Worst case: pure white artwork under the gradient's darkest part.
        let bg = over(scrim, Color32::WHITE);
        assert!(contrast(p.on_art, bg) >= 4.5, "{}", contrast(p.on_art, bg));
    }

    #[test]
    fn accent_parse_round_trips() {
        for a in Accent::ALL {
            assert_eq!(Accent::parse(a.as_str()), Some(a));
        }
    }
}
