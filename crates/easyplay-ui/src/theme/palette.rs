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
            Accent::Tide => [0x0A7D6F, 0x097266, 0x086A5E, 0xFFFFFF, 0x0A7D6F],
            Accent::Ember => [0xC2410C, 0xB53C0B, 0xA8380A, 0xFFFFFF, 0xC2410C],
            Accent::Iris => [0x4338CA, 0x5046E0, 0x3730A3, 0xFFFFFF, 0x4338CA],
            Accent::Moss => [0x4D7C0F, 0x46720E, 0x3F650C, 0xFFFFFF, 0x4D7C0F],
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
    /// Boundaries that alone identify a control: form fields, unselected toggles.
    pub border_strong: Color32,
    /// A translucent edge that works on any plane and over artwork: cover edges, quiet
    /// controls identified by their label, menu and sheet outlines.
    pub hairline: Color32,
    /// Fill of a quiet control or row under the pointer, over any plane.
    pub fill_hover: Color32,
    /// Fill of the selected row or segment, over any plane.
    pub fill_selected: Color32,
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
            bg_sunken: hex(0x09090B),
            bg_base: hex(0x0E0E10),
            bg_surface: hex(0x141416),
            bg_raised: hex(0x1C1C1F),
            bg_overlay: hex(0x232327),
            border_subtle: hex(0x27272B),
            border_strong: hex(0x72727A),
            hairline: hex_a(0xF3F3F5, alpha_u8(0.09)),
            fill_hover: hex_a(0xF3F3F5, alpha_u8(0.05)),
            fill_selected: hex_a(0xF3F3F5, alpha_u8(0.09)),
            text_primary: hex(0xF3F3F5),
            text_secondary: hex(0xB5B5BC),
            text_muted: hex(0x9999A1),
            text_disabled: hex(0x505057),
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
            backdrop: hex_a(0x000000, alpha_u8(0.60)),
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
            bg_sunken: hex(0xEBEBEF),
            bg_base: hex(0xF5F5F7),
            bg_surface: hex(0xFFFFFF),
            bg_raised: hex(0xFFFFFF),
            bg_overlay: hex(0xFFFFFF),
            border_subtle: hex(0xE3E3E8),
            border_strong: hex(0x80808A),
            hairline: hex_a(0x121214, alpha_u8(0.10)),
            fill_hover: hex_a(0x121214, alpha_u8(0.04)),
            fill_selected: hex_a(0x121214, alpha_u8(0.07)),
            text_primary: hex(0x121214),
            text_secondary: hex(0x494951),
            text_muted: hex(0x5C5C66),
            text_disabled: hex(0xA9A9B2),
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
            shadow: hex_a(0x000000, alpha_u8(0.14)),
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
            hairline: white,
            fill_hover: hex(0x1F1F1F),
            fill_selected: hex(0x2B2B2B),
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

/// The colours of a generated cover (UI_DESIGN_SYSTEM §7.2): a quiet wash from `top` to
/// `bottom` and the tone-on-tone `ink` of its monogram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceholderColors {
    /// Top of the wash.
    pub top: Color32,
    /// Bottom of the wash.
    pub bottom: Color32,
    /// The monogram.
    pub ink: Color32,
}

/// Hues a generated cover may take, in turns: evenly spread, none of them the accent's.
const PLACEHOLDER_HUES: [f32; 10] = [0.02, 0.08, 0.13, 0.27, 0.40, 0.55, 0.61, 0.68, 0.77, 0.90];

/// The colours of the generated cover for `seed` (a hash of the game's name). Saturation and
/// lightness are fixed per theme, so a shelf of generated covers reads as one family.
pub fn placeholder(seed: u64, kind: ThemeKind) -> PlaceholderColors {
    let hue = PLACEHOLDER_HUES[(seed % PLACEHOLDER_HUES.len() as u64) as usize];
    // A small per-name drift keeps two games on the same hue from being identical.
    let drift = ((seed >> 16) % 5) as f32 * 0.006;
    let h = hue + drift;
    match kind {
        ThemeKind::Dark => PlaceholderColors {
            top: hsl(h, 0.30, 0.24),
            bottom: hsl(h + 0.03, 0.34, 0.13),
            ink: hsl(h, 0.30, 0.36),
        },
        ThemeKind::Light => PlaceholderColors {
            top: hsl(h, 0.36, 0.84),
            bottom: hsl(h + 0.03, 0.34, 0.74),
            ink: hsl(h, 0.30, 0.62),
        },
        ThemeKind::HighContrast => PlaceholderColors {
            top: hex(0x000000),
            bottom: hex(0x000000),
            ink: hex(0xFFFFFF),
        },
    }
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

/// Minimum contrast for text under 18 px regular / 14 px bold (ACCESSIBILITY §3).
pub const MIN_TEXT: f32 = 4.5;
/// Minimum contrast for large text, meaningful icons and control boundaries.
pub const MIN_NON_TEXT: f32 = 3.0;

/// One allowed (foreground role, background role) combination and its threshold.
#[derive(Clone, Debug)]
pub struct Pair {
    /// Foreground role name.
    pub fg_role: &'static str,
    /// Background role name.
    pub bg_role: &'static str,
    /// Foreground colour (may be translucent; composited over `bg`).
    pub fg: Color32,
    /// Opaque background colour.
    pub bg: Color32,
    /// Minimum ratio.
    pub min: f32,
}

/// The table of allowed combinations (ACCESSIBILITY §3, task A11Y-002): every foreground
/// role the widgets paint on every background role they paint it on. `text.disabled` is
/// exempt; decorative borders (`border.subtle`) are not listed.
pub fn allowed_pairs(p: &Palette) -> Vec<Pair> {
    let mut v = Vec::new();
    let mut push = |fg_role, fg, bg_role, bg, min| {
        v.push(Pair {
            fg_role,
            bg_role,
            fg,
            bg,
            min,
        });
    };
    // Planes text and controls sit on. `bg.sunken` only carries text in the log viewer.
    let planes = [
        ("bg.base", p.bg_base),
        ("bg.surface", p.bg_surface),
        ("bg.raised", p.bg_raised),
        ("bg.overlay", p.bg_overlay),
    ];
    for (bg_role, bg) in planes {
        push("text.primary", p.text_primary, bg_role, bg, MIN_TEXT);
        push("text.secondary", p.text_secondary, bg_role, bg, MIN_TEXT);
        push("text.muted", p.text_muted, bg_role, bg, MIN_TEXT);
        // Links and selected labels are accent-coloured text.
        push("accent (text)", p.accent, bg_role, bg, MIN_TEXT);
        // Semantic colours appear as inline error / warning / status text.
        push("danger", p.danger, bg_role, bg, MIN_TEXT);
        push("warning", p.warning, bg_role, bg, MIN_TEXT);
        push("success", p.success, bg_role, bg, MIN_TEXT);
        push("info", p.info, bg_role, bg, MIN_TEXT);
        push("focus", p.focus, bg_role, bg, MIN_NON_TEXT);
        push("border.strong", p.border_strong, bg_role, bg, MIN_NON_TEXT);
        for (i, t) in p.tags.iter().enumerate() {
            push(TAG_ROLES[i], *t, bg_role, bg, MIN_NON_TEXT);
        }
    }
    push(
        "text.primary",
        p.text_primary,
        "bg.sunken",
        p.bg_sunken,
        MIN_TEXT,
    );
    push(
        "text.secondary",
        p.text_secondary,
        "bg.sunken",
        p.bg_sunken,
        MIN_TEXT,
    );
    push(
        "text.muted",
        p.text_muted,
        "bg.sunken",
        p.bg_sunken,
        MIN_TEXT,
    );
    // Selected rows, sidebar items and chips: `accent.subtle` over the surface they fill.
    for (bg_role, under) in [
        ("accent.subtle/bg.surface", p.bg_surface),
        ("accent.subtle/bg.base", p.bg_base),
        ("accent.subtle/bg.raised", p.bg_raised),
    ] {
        let bg = over(p.accent_subtle, under);
        push("text.primary", p.text_primary, bg_role, bg, MIN_TEXT);
        push("text.secondary", p.text_secondary, bg_role, bg, MIN_TEXT);
        push("text.muted", p.text_muted, bg_role, bg, MIN_TEXT);
        push("accent (icon)", p.accent, bg_role, bg, MIN_NON_TEXT);
    }
    // Quiet fills: the row or control under the pointer on every plane, and the selected
    // row or segment on the planes that carry lists.
    let fills = [
        ("fill.hover/bg.base", p.fill_hover, p.bg_base),
        ("fill.hover/bg.surface", p.fill_hover, p.bg_surface),
        ("fill.hover/bg.raised", p.fill_hover, p.bg_raised),
        ("fill.hover/bg.overlay", p.fill_hover, p.bg_overlay),
        ("fill.selected/bg.base", p.fill_selected, p.bg_base),
        ("fill.selected/bg.surface", p.fill_selected, p.bg_surface),
        ("fill.selected/bg.raised", p.fill_selected, p.bg_raised),
    ];
    for (bg_role, fill, under) in fills {
        let bg = over(fill, under);
        push("text.primary", p.text_primary, bg_role, bg, MIN_TEXT);
        push("text.secondary", p.text_secondary, bg_role, bg, MIN_TEXT);
        push("text.muted", p.text_muted, bg_role, bg, MIN_TEXT);
        push("accent (icon)", p.accent, bg_role, bg, MIN_NON_TEXT);
    }
    // Filled accent buttons in every state.
    for (bg_role, bg) in [
        ("accent", p.accent),
        ("accent.hover", p.accent_hover),
        ("accent.pressed", p.accent_pressed),
    ] {
        push("text.on_accent", p.text_on_accent, bg_role, bg, MIN_TEXT);
    }
    // The primary fill must stand out from the page (non-text contrast of the control).
    push(
        "accent (fill)",
        p.accent,
        "bg.base",
        p.bg_base,
        MIN_NON_TEXT,
    );
    push(
        "accent (fill)",
        p.accent,
        "bg.overlay",
        p.bg_overlay,
        MIN_NON_TEXT,
    );
    v
}

const TAG_ROLES: [&str; 8] = [
    "tag.0", "tag.1", "tag.2", "tag.3", "tag.4", "tag.5", "tag.6", "tag.7",
];

/// Every pair of [`allowed_pairs`] below its threshold, for every theme and accent, as
/// readable lines (empty when the palette passes).
pub fn contrast_failures() -> Vec<String> {
    let mut failures = Vec::new();
    for kind in ThemeKind::ALL {
        for accent in Accent::ALL {
            let p = Palette::new(kind, accent);
            for pair in allowed_pairs(&p) {
                let r = contrast(pair.fg, pair.bg);
                if r < pair.min {
                    failures.push(format!(
                        "{kind:?}/{accent:?}: {} on {} = {r:.2} < {}",
                        pair.fg_role, pair.bg_role, pair.min
                    ));
                }
            }
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_of_black_and_white() {
        let r = contrast(Color32::WHITE, Color32::BLACK);
        assert!((r - 21.0).abs() < 0.01, "{r}");
        assert!((contrast(Color32::BLACK, Color32::BLACK) - 1.0).abs() < 1e-4);
    }

    /// A11Y-002: the whole matrix, every theme, every accent.
    #[test]
    fn every_theme_and_accent_meets_contrast_minimums() {
        let failures = contrast_failures();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn the_matrix_covers_every_theme_and_accent() {
        let p = Palette::new(ThemeKind::Dark, Accent::Tide);
        // 4 planes × (9 + 8 tags) + sunken 3 + subtle 3 × 4 + fills 7 × 4 + accent fills 3 + 2.
        assert_eq!(allowed_pairs(&p).len(), 4 * 18 + 3 + 12 + 28 + 3 + 2);
    }

    #[test]
    fn a_failing_pair_is_reported() {
        let mut p = Palette::new(ThemeKind::Light, Accent::Tide);
        p.text_muted = hex(0xC0C0C0);
        let low: Vec<_> = allowed_pairs(&p)
            .into_iter()
            .filter(|x| contrast(x.fg, x.bg) < x.min)
            .collect();
        assert!(low.iter().any(|x| x.fg_role == "text.muted"));
    }

    /// Controls over artwork (favourite, more, the state glyph of a missing game) sit on a
    /// scrim disc that keeps their glyph at 3:1 over a pure white image and over a pure black
    /// one (ACCESSIBILITY §3). Titles are never painted over artwork.
    #[test]
    fn the_scrim_keeps_glyphs_legible_over_any_art() {
        use crate::theme::tokens::alpha::SCRIM;
        for kind in ThemeKind::ALL {
            let p = Palette::new(kind, Accent::Tide);
            for art in [Color32::WHITE, Color32::BLACK, hex(0xFFE600), hex(0x7DD3FC)] {
                let bg = over(fade(p.scrim, SCRIM), art);
                let r = contrast(p.on_art, bg);
                assert!(r >= MIN_NON_TEXT, "{kind:?}: on_art over {art:?} = {r:.2}");
            }
        }
    }

    /// The generated cover's monogram is decoration (the title is beneath the cover), but it
    /// must stay visible: its ink differs from both ends of the wash.
    #[test]
    fn placeholder_ink_is_visible_and_stable() {
        for kind in ThemeKind::ALL {
            for seed in [0_u64, 1, 7, 12_345, u64::MAX] {
                let c = placeholder(seed, kind);
                assert_eq!(c, placeholder(seed, kind));
                assert!(contrast(c.ink, c.top) > 1.15, "{kind:?} {seed}");
                assert!(contrast(c.ink, c.bottom) > 1.15, "{kind:?} {seed}");
            }
        }
        assert_ne!(
            placeholder(1, ThemeKind::Dark).top,
            placeholder(2, ThemeKind::Dark).top
        );
    }

    #[test]
    fn high_contrast_has_no_translucency() {
        let p = Palette::new(ThemeKind::HighContrast, Accent::Ember);
        for c in [p.bg_base, p.bg_overlay, p.accent_subtle, p.backdrop] {
            assert_eq!(c.a(), 255);
        }
        assert!(!p.allows_translucency());
        assert_eq!(p.focus, hex(0xFFE600));
        assert_eq!(p.text_primary, Color32::WHITE);
        assert_eq!(p.bg_base, Color32::BLACK);
    }

    #[test]
    fn accent_parse_round_trips() {
        for a in Accent::ALL {
            assert_eq!(Accent::parse(a.as_str()), Some(a));
        }
    }
}
