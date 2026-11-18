//! Colour roles for the dark, light and high-contrast themes, the accent ramps, the tag dots
//! and the poster papers (UI_DESIGN_SYSTEM §2). The only place in the crate where colour
//! values are written down.

use egui::Color32;

/// `#RRGGBB` as an opaque colour.
const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// `#RRGGBB` at `fraction` opacity (0–1).
fn hex_a(rgb: u32, fraction: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (rgb >> 16) as u8,
        (rgb >> 8) as u8,
        rgb as u8,
        alpha_u8(fraction),
    )
}

/// Fraction (0–1) to an 8-bit alpha.
fn alpha_u8(fraction: f32) -> u8 {
    (fraction.clamp(0.0, 1.0) * 255.0).round() as u8
}

const WHITE: u32 = 0xFFFFFF;

/// Colours that are the same in every theme: what sits on or under pictures.
pub mod fixed {
    use egui::Color32;

    /// Behind a picture or video that is still loading, in the media viewer.
    pub const MEDIA_STAGE: Color32 = Color32::from_rgb(0x14, 0x15, 0x18);
    /// A message picture that could not be shown, and its icon.
    pub const PICTURE_MISSING: Color32 = Color32::from_gray(40);
    pub const PICTURE_MISSING_ICON: Color32 = Color32::from_gray(160);
    /// The coral of the live mark on the black stage chips.
    pub const LIVE_ON_DARK: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x7C);
    /// A missing game's artwork: half brightness, under a grey veil.
    pub const MISSING_ART_TINT: Color32 = Color32::from_gray(128);
    pub const MISSING_ART_VEIL: Color32 = Color32::from_gray(52);
}
const BLACK: u32 = 0x000000;

/// Which palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeKind {
    /// Default.
    Dark,
    /// Light.
    Light,
    /// Black planes, white text, strong outlines.
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

/// One accent as a ramp from light to dark, and the ink that is read on its middle steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ramp {
    /// 300: accent text on dark planes.
    pub a300: Color32,
    /// 400: the fill under the pointer.
    pub a400: Color32,
    /// 500: the fill.
    pub a500: Color32,
    /// 600: the fill while pressed.
    pub a600: Color32,
    /// 700: fill and accent text on light planes.
    pub a700: Color32,
    /// Text on the 400–600 fills.
    pub ink: Color32,
}

/// Accent alternatives (§2.4). An accent changes the fill, the accent text, the accent mark
/// and the tint, nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Accent {
    /// Teal (default).
    #[default]
    Tide,
    /// Orange.
    Ember,
    /// Violet.
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

    /// The accent's ramp.
    pub const fn ramp(self) -> Ramp {
        let [a300, a400, a500, a600, a700, ink] = match self {
            Accent::Tide => [0x7FE9DA, 0x55DFCC, 0x2ED3BC, 0x1FB9A4, 0x0A7164, 0x04211D],
            Accent::Ember => [0xFFB59A, 0xFF9C7A, 0xFF8256, 0xEA6B40, 0xA83C16, 0x2A0E04],
            Accent::Iris => [0xC9BEFF, 0xB5A7FF, 0x9F8DFF, 0x8773F0, 0x5740D0, 0x120A33],
            Accent::Moss => [0xB7E08A, 0xA3D56E, 0x8CC850, 0x76B23C, 0x386E12, 0x14240A],
        };
        Ramp {
            a300: hex(a300),
            a400: hex(a400),
            a500: hex(a500),
            a600: hex(a600),
            a700: hex(a700),
            ink: hex(ink),
        }
    }

    /// The colour of the accent's swatch in Settings (its 500 step, in every theme).
    pub const fn swatch(self) -> Color32 {
        self.ramp().a500
    }
}

/// A drop shadow: offset down, blur and colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shade {
    /// Offset downwards, in points.
    pub y: i8,
    /// Blur width, in points.
    pub blur: u8,
    /// Colour, with its opacity.
    pub color: Color32,
}

impl Shade {
    /// As an egui shadow.
    pub const fn shadow(self) -> egui::Shadow {
        egui::Shadow {
            offset: [0, self.y],
            blur: self.blur,
            spread: 0,
            color: self.color,
        }
    }
}

/// Every colour role of §2. Widgets read these through [`crate::theme::Theme`].
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    /// Which theme this is.
    pub kind: ThemeKind,
    /// The wall: every page.
    pub bg: Color32,
    /// Menus, dialogs, toasts.
    pub raised: Color32,
    /// Text fields, the log view.
    pub well: Color32,
    /// Text: primary.
    pub t1: Color32,
    /// Text: secondary.
    pub t2: Color32,
    /// Text: quiet (labels, counts, help).
    pub t3: Color32,
    /// Text: disabled.
    pub t4: Color32,
    /// Fill at rest: boxed buttons, the search field, a segmented track, a row under the
    /// pointer. Translucent, so one value works on every plane.
    pub o1: Color32,
    /// Fill under the pointer: buttons, menu items.
    pub o2: Color32,
    /// Fill while pressed; the off track of a switch; an empty bar.
    pub o3: Color32,
    /// Hairline: rules under headers and table rows, outlines of menus and fields.
    pub line: Color32,
    /// A stronger line: the chart's base, dashed outlines.
    pub line2: Color32,
    /// The accent fill: Play, a switch that is on, checks.
    pub fill: Color32,
    /// The accent fill under the pointer.
    pub fill_hover: Color32,
    /// The accent fill while pressed.
    pub fill_pressed: Color32,
    /// Text and glyphs on the accent fill.
    pub on_fill: Color32,
    /// Accent-coloured text: links, the session clock.
    pub accent_text: Color32,
    /// Accent-coloured marks: the focus ring, the tab underline, selection.
    pub accent_mark: Color32,
    /// Selected row, chosen option.
    pub tint: Color32,
    /// A missing program, a warning.
    pub amber: Color32,
    /// Fill of an amber note.
    pub amber_tint: Color32,
    /// A problem, a destructive action.
    pub coral: Color32,
    /// Fill of a coral note; a warn button under the pointer.
    pub coral_tint: Color32,
    /// Text on a coral fill.
    pub on_coral: Color32,
    /// The inner line of a cover.
    pub edge: Color32,
    /// Behind dialogs.
    pub scrim: Color32,
    /// Tooltip plane.
    pub tip_bg: Color32,
    /// Tooltip text.
    pub tip_fg: Color32,
    /// The thumb of a segmented control.
    pub thumb: Color32,
    /// The knob of a switch that is off.
    pub knob: Color32,
    /// Shadow of a cover under the pointer.
    pub shade_lift: Shade,
    /// Shadow of a menu or toast.
    pub shade_pop: Shade,
    /// Shadow of a dialog.
    pub shade_dialog: Shade,
    /// Shadow of the cover on the game page (the same in every theme).
    pub shade_cover: Shade,
    /// Disc behind a badge on a cover (favourite); its glyph is [`Palette::on_art`].
    pub badge: Color32,
    /// Glyphs on [`Palette::badge`], and the Play disc of a running game (independent of
    /// the theme: covers keep their own colours).
    pub on_art: Color32,
    /// The glyph on the Play disc of a running game.
    pub on_art_ink: Color32,
    /// The Play disc of a game that cannot start now.
    pub disc_off: Color32,
    /// The glyph on [`Palette::disc_off`].
    pub on_disc_off: Color32,
    /// The ring that keeps the Play disc apart from a light cover.
    pub disc_ring: Color32,
    /// The six tag dots.
    pub tags: [Color32; 6],
}

/// The tag dots on dark planes and on light ones.
const TAG_DOTS: [(u32, u32); 6] = [
    (0x7EC8FF, 0x2E7FB8),
    (0xFFB86C, 0xA8661A),
    (0xBEAAFF, 0x6E55C8),
    (0x8CDCAA, 0x2F8A5B),
    (0xFF96AA, 0xC24A66),
    (0xE6D678, 0x857A1E),
];

impl Palette {
    /// The palette for `kind` with `accent`.
    pub fn new(kind: ThemeKind, accent: Accent) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(accent),
            ThemeKind::Light => Self::light(accent),
            ThemeKind::HighContrast => Self::high_contrast(accent),
        }
    }

    fn dark(accent: Accent) -> Self {
        let a = accent.ramp();
        let shade = |y, blur, opacity| Shade {
            y,
            blur,
            color: hex_a(BLACK, opacity),
        };
        Self {
            kind: ThemeKind::Dark,
            bg: hex(0x0E0F10),
            raised: hex(0x1B1D1F),
            well: hex(0x151718),
            t1: hex(0xECEDEE),
            t2: hex(0xA7ABAF),
            t3: hex(0x888D92),
            t4: hex(0x4C5054),
            o1: hex_a(WHITE, 0.05),
            o2: hex_a(WHITE, 0.09),
            o3: hex_a(WHITE, 0.14),
            line: hex_a(WHITE, 0.09),
            line2: hex_a(WHITE, 0.18),
            fill: a.a500,
            fill_hover: a.a400,
            fill_pressed: a.a600,
            on_fill: a.ink,
            accent_text: a.a300,
            accent_mark: a.a500,
            tint: fade(a.a500, 0.14),
            amber: hex(0xF0B45A),
            amber_tint: hex_a(0xF0B45A, 0.12),
            coral: hex(0xFF7262),
            coral_tint: hex_a(0xFF7262, 0.12),
            on_coral: hex(0x2B0A06),
            edge: hex_a(WHITE, 0.07),
            scrim: hex_a(BLACK, 0.56),
            tip_bg: hex(0x2B2E31),
            tip_fg: hex(0xECEDEE),
            thumb: hex_a(WHITE, 0.14),
            knob: hex(0xA7ABAF),
            shade_lift: shade(12, 28, 0.5),
            shade_pop: shade(12, 32, 0.5),
            shade_dialog: shade(24, 64, 0.6),
            shade_cover: shade(18, 44, 0.4),
            badge: hex_a(BLACK, 0.62),
            on_art: hex(0xECEDEE),
            on_art_ink: hex(0x0E0F10),
            disc_off: hex(0x33373A),
            on_disc_off: hex(0x8A8F94),
            disc_ring: hex_a(BLACK, 0.3),
            tags: TAG_DOTS.map(|(dark, _)| hex(dark)),
        }
    }

    fn light(accent: Accent) -> Self {
        let a = accent.ramp();
        let soot = 0x141618;
        let shade = |y, blur, opacity| Shade {
            y,
            blur,
            color: hex_a(soot, opacity),
        };
        Self {
            kind: ThemeKind::Light,
            bg: hex(0xF4F4F2),
            raised: hex(WHITE),
            well: hex(WHITE),
            t1: hex(0x131415),
            t2: hex(0x474B4F),
            t3: hex(0x5C6166),
            t4: hex(0xA9ADB1),
            o1: hex_a(BLACK, 0.045),
            o2: hex_a(BLACK, 0.08),
            o3: hex_a(BLACK, 0.13),
            line: hex_a(BLACK, 0.10),
            line2: hex_a(BLACK, 0.20),
            fill: a.a700,
            fill_hover: mix(a.a700, hex(WHITE), 0.10),
            fill_pressed: mix(a.a700, hex(BLACK), 0.12),
            on_fill: hex(WHITE),
            accent_text: a.a700,
            accent_mark: a.a700,
            tint: fade(a.a500, 0.14),
            amber: hex(0x8A5A00),
            amber_tint: hex_a(0xBE7D00, 0.14),
            coral: hex(0xC8382C),
            coral_tint: hex_a(0xC8382C, 0.10),
            on_coral: hex(WHITE),
            edge: hex_a(BLACK, 0.12),
            scrim: hex_a(soot, 0.36),
            tip_bg: hex(0x232527),
            tip_fg: hex(0xF4F4F2),
            thumb: hex(WHITE),
            knob: hex(WHITE),
            shade_lift: shade(12, 28, 0.24),
            shade_pop: shade(12, 32, 0.18),
            shade_dialog: shade(24, 64, 0.26),
            shade_cover: Shade {
                y: 18,
                blur: 44,
                color: hex_a(BLACK, 0.4),
            },
            badge: hex_a(BLACK, 0.62),
            on_art: hex(0xECEDEE),
            on_art_ink: hex(0x0E0F10),
            disc_off: hex(0x33373A),
            on_disc_off: hex(0x8A8F94),
            disc_ring: hex_a(BLACK, 0.3),
            tags: TAG_DOTS.map(|(_, light)| hex(light)),
        }
    }

    fn high_contrast(accent: Accent) -> Self {
        let a = accent.ramp();
        Self {
            kind: ThemeKind::HighContrast,
            bg: hex(BLACK),
            raised: hex(0x121212),
            well: hex(BLACK),
            t1: hex(WHITE),
            t2: hex(0xEDEDED),
            t3: hex(0xD2D2D2),
            t4: hex(0x8E8E8E),
            o1: hex_a(WHITE, 0.10),
            o2: hex_a(WHITE, 0.18),
            o3: hex_a(WHITE, 0.30),
            line: hex_a(WHITE, 0.50),
            line2: hex_a(WHITE, 0.80),
            accent_mark: a.a300,
            thumb: hex_a(WHITE, 0.30),
            knob: hex(WHITE),
            scrim: hex_a(BLACK, 0.80),
            tip_bg: hex(WHITE),
            tip_fg: hex(BLACK),
            ..Self::dark(accent)
        }
    }

    /// The colour of tag dot `index` (wraps at 6).
    pub fn tag(&self, index: u8) -> Color32 {
        self.tags[usize::from(index) % self.tags.len()]
    }

    /// The dot of the tag called `name`: a tag keeps its colour for as long as it keeps its
    /// name.
    pub fn tag_dot(&self, name: &str) -> Color32 {
        self.tags[(fnv1a(name) % self.tags.len() as u32) as usize]
    }

    /// Whether controls carry a 1 pt outline (the high-contrast theme).
    pub const fn outlined(&self) -> bool {
        matches!(self.kind, ThemeKind::HighContrast)
    }
}

/// FNV-1a over the UTF-16 code units of `s`: what gives a title its paper and a tag its dot.
pub fn fnv1a(s: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for unit in s.encode_utf16() {
        h ^= u32::from(unit);
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// The colours of a poster (§7.2): its paper and the ink its title is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paper {
    /// The cover's ground.
    pub paper: Color32,
    /// Title and mark.
    pub ink: Color32,
}

/// Forest, oxblood, navy, plum, ochre, slate, bone, olive, rose, clay. Posters keep their
/// own colours in every theme.
const PAPERS: [(u32, u32); 10] = [
    (0x1F3A2B, 0x9FD6AE),
    (0x4A1D1C, 0xF0A79A),
    (0x1B2A4A, 0x9DB8F2),
    (0x3A2145, 0xD6A8EA),
    (0x4A3A14, 0xEBCB73),
    (0x2A3338, 0xB4C7D0),
    (0xD9D2C3, 0x1B1B1B),
    (0x343A16, 0xCDD68A),
    (0x5A2338, 0xF7B3CB),
    (0x6B3A22, 0xF5C9A8),
];

/// How many papers there are.
pub const PAPER_COUNT: usize = PAPERS.len();

/// Paper `index` (wraps).
pub fn paper(index: usize) -> Paper {
    let (paper, ink) = PAPERS[index % PAPERS.len()];
    Paper {
        paper: hex(paper),
        ink: hex(ink),
    }
}

/// The paper of the game called `title`: it keeps its colours for as long as it keeps its
/// name.
pub fn paper_for(title: &str) -> Paper {
    paper((fnv1a(&format!("ink{title}")) % PAPERS.len() as u32) as usize)
}

/// A cover whose game cannot be found: about 15 % of its saturation, half its brightness.
pub fn dimmed(c: Color32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
    // CSS `saturate(.15) brightness(.5)`: mix towards the luma, then halve.
    let luma = 0.213 * r + 0.715 * g + 0.072 * b;
    let ch = |v: f32| (((luma + (v - luma) * 0.15) * 0.5).round()).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(r), ch(g), ch(b), a)
}

/// `c` with its alpha multiplied by `fraction`.
pub fn fade(c: Color32, fraction: f32) -> Color32 {
    c.gamma_multiply(fraction.clamp(0.0, 1.0))
}

/// Mix `a` towards `b` by `t` (0–1) in gamma space.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// An opaque colour from hue (0–1), saturation (0–1) and lightness (0–1). Used for the
/// synthetic artwork of the preview library; kept here so that colour arithmetic stays
/// inside `theme`.
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
/// role the widgets paint on every background they paint it on. `t4` (disabled) is exempt;
/// hairlines are decoration and are not listed.
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
    // The planes, and every text role on them.
    let planes = [("bg", p.bg), ("raised", p.raised), ("well", p.well)];
    for (bg_role, bg) in planes {
        push("t1", p.t1, bg_role, bg, MIN_TEXT);
        push("t2", p.t2, bg_role, bg, MIN_TEXT);
        push("t3", p.t3, bg_role, bg, MIN_TEXT);
        push("accent text", p.accent_text, bg_role, bg, MIN_TEXT);
        push("coral", p.coral, bg_role, bg, MIN_TEXT);
        push("amber", p.amber, bg_role, bg, MIN_TEXT);
        push("accent mark", p.accent_mark, bg_role, bg, MIN_NON_TEXT);
        push("fill", p.fill, bg_role, bg, MIN_NON_TEXT);
    }
    // Tag dots stand on the page and in menus.
    for (bg_role, bg) in [("bg", p.bg), ("raised", p.raised)] {
        for (i, t) in p.tags.iter().enumerate() {
            push(TAG_ROLES[i], *t, bg_role, bg, MIN_NON_TEXT);
        }
    }
    // Quiet fills: a boxed button or a row under the pointer (o1), a button under the
    // pointer (o2), a pressed button (o3); on the page and on raised planes.
    for (under_role, under) in [("bg", p.bg), ("raised", p.raised)] {
        for (fill_role, fill) in [("o1", p.o1), ("o2", p.o2), ("o3", p.o3)] {
            let bg = over(fill, under);
            let role: &'static str = match (fill_role, under_role) {
                ("o1", "bg") => "o1/bg",
                ("o2", "bg") => "o2/bg",
                ("o3", "bg") => "o3/bg",
                ("o1", _) => "o1/raised",
                ("o2", _) => "o2/raised",
                _ => "o3/raised",
            };
            push("t1", p.t1, role, bg, MIN_TEXT);
            if fill_role == "o1" {
                push("t2", p.t2, role, bg, MIN_TEXT);
                push("t3", p.t3, role, bg, MIN_TEXT);
            }
        }
    }
    // A selected row, a chosen option.
    for (bg_role, under) in [("tint/bg", p.bg), ("tint/raised", p.raised)] {
        let bg = over(p.tint, under);
        push("t1", p.t1, bg_role, bg, MIN_TEXT);
        push("t2", p.t2, bg_role, bg, MIN_TEXT);
        push("t3", p.t3, bg_role, bg, MIN_TEXT);
    }
    // The segmented thumb sits on the track.
    let track = over(p.o1, p.bg);
    push("t1", p.t1, "thumb", over(p.thumb, track), MIN_TEXT);
    // Notes above the tabs.
    let amber_note = over(p.amber_tint, p.bg);
    let coral_note = over(p.coral_tint, p.bg);
    push("t1", p.t1, "amber tint", amber_note, MIN_TEXT);
    push("amber", p.amber, "amber tint", amber_note, MIN_NON_TEXT);
    push("t1", p.t1, "coral tint", coral_note, MIN_TEXT);
    push("t3", p.t3, "coral tint", coral_note, MIN_TEXT);
    push("coral", p.coral, "coral tint", coral_note, MIN_NON_TEXT);
    // Filled controls in every state.
    for (bg_role, bg) in [
        ("fill", p.fill),
        ("fill hover", p.fill_hover),
        ("fill pressed", p.fill_pressed),
    ] {
        push("on fill", p.on_fill, bg_role, bg, MIN_TEXT);
    }
    push("on coral", p.on_coral, "coral", p.coral, MIN_TEXT);
    push("tip fg", p.tip_fg, "tip bg", p.tip_bg, MIN_TEXT);
    // Glyphs on covers.
    push("on art ink", p.on_art_ink, "on art", p.on_art, MIN_NON_TEXT);
    push(
        "on disc off",
        p.on_disc_off,
        "disc off",
        p.disc_off,
        MIN_NON_TEXT,
    );
    v
}

const TAG_ROLES: [&str; 6] = ["tag 0", "tag 1", "tag 2", "tag 3", "tag 4", "tag 5"];

/// Where the design's own values fall short of [`MIN_TEXT`], and by how much it is known to:
/// quiet text (`t3`) on a row that is under the pointer or chosen, in the dark theme. The
/// values are the design's and are kept; the floor keeps them from slipping further.
/// (ACCESSIBILITY §3 lists them.)
pub const KNOWN_SHORTFALLS: [(&str, &str, f32); 3] = [
    ("t3", "o1/raised", 4.3),
    ("t3", "tint/bg", 4.4),
    ("t3", "tint/raised", 3.75),
];

/// The least ratio `pair` must reach.
fn floor(pair: &Pair) -> f32 {
    KNOWN_SHORTFALLS
        .iter()
        .find(|(fg, bg, _)| *fg == pair.fg_role && *bg == pair.bg_role)
        .map_or(pair.min, |(_, _, floor)| floor.min(pair.min))
}

/// Every pair of [`allowed_pairs`] below its threshold, for every theme and accent, as
/// readable lines (empty when the palette passes).
pub fn contrast_failures() -> Vec<String> {
    let mut failures = Vec::new();
    for kind in ThemeKind::ALL {
        for accent in Accent::ALL {
            let p = Palette::new(kind, accent);
            for pair in allowed_pairs(&p) {
                let r = contrast(pair.fg, pair.bg);
                let min = floor(&pair);
                if r < min {
                    failures.push(format!(
                        "{kind:?}/{accent:?}: {} on {} = {r:.2} < {min}",
                        pair.fg_role, pair.bg_role
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

    /// The shortfalls are real (so the list is not stale) and confined to the dark theme.
    #[test]
    fn the_known_shortfalls_are_the_only_ones() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in ThemeKind::ALL {
            for accent in Accent::ALL {
                let p = Palette::new(kind, accent);
                for pair in allowed_pairs(&p) {
                    if contrast(pair.fg, pair.bg) < pair.min {
                        assert_eq!(
                            kind,
                            ThemeKind::Dark,
                            "{} on {}",
                            pair.fg_role,
                            pair.bg_role
                        );
                        seen.insert((pair.fg_role, pair.bg_role));
                    }
                }
            }
        }
        let known: std::collections::BTreeSet<_> =
            KNOWN_SHORTFALLS.iter().map(|(f, b, _)| (*f, *b)).collect();
        assert_eq!(seen, known);
    }

    #[test]
    fn a_failing_pair_is_reported() {
        let mut p = Palette::new(ThemeKind::Light, Accent::Tide);
        p.t3 = hex(0xC0C0C0);
        let low: Vec<_> = allowed_pairs(&p)
            .into_iter()
            .filter(|x| contrast(x.fg, x.bg) < x.min)
            .collect();
        assert!(low.iter().any(|x| x.fg_role == "t3"));
    }

    /// The figures the design states for the text roles on the page.
    #[test]
    fn text_roles_have_the_stated_contrast() {
        let d = Palette::new(ThemeKind::Dark, Accent::Tide);
        assert!((contrast(d.t1, d.bg) - 16.4).abs() < 0.15);
        assert!((contrast(d.t2, d.bg) - 8.3).abs() < 0.15);
        assert!((contrast(d.t3, d.bg) - 5.7).abs() < 0.15);
        assert!((contrast(d.t3, d.raised) - 5.1).abs() < 0.15);
        let l = Palette::new(ThemeKind::Light, Accent::Tide);
        assert!((contrast(l.t1, l.bg) - 16.7).abs() < 0.15);
        assert!((contrast(l.t2, l.bg) - 8.0).abs() < 0.15);
        assert!((contrast(l.t3, l.bg) - 5.7).abs() < 0.15);
    }

    /// Ink on paper is 6.1:1 or better for all ten posters.
    #[test]
    fn every_poster_is_legible() {
        for i in 0..PAPER_COUNT {
            let p = paper(i);
            let r = contrast(p.ink, p.paper);
            assert!(r >= 6.1, "paper {i}: {r:.2}");
        }
    }

    /// The hash is the prototype's (FNV-1a over UTF-16 code units), so a game has the same
    /// paper here as in the design's renders. Values read from the prototype.
    #[test]
    fn titles_keep_the_papers_of_the_design() {
        assert_eq!(fnv1a(""), 2_166_136_261);
        assert_eq!(fnv1a("a"), 0xE40C_292C);
        let index = |t: &str| (fnv1a(&format!("ink{t}")) % 10) as usize;
        // Lantern Keep is on bone, Low Tide Motel on navy, Driftwood on clay.
        assert_eq!(index("Lantern Keep"), 6);
        assert_eq!(index("Low Tide Motel"), 2);
        assert_eq!(index("Driftwood"), 9);
        assert_eq!(paper_for("Lantern Keep").paper, hex(0xD9D2C3));
        // Outside ASCII the units are UTF-16, as in the prototype.
        assert_eq!(fnv1a("é"), fnv1a("\u{e9}"));
        assert_ne!(fnv1a("é"), fnv1a("e"));
    }

    #[test]
    fn a_missing_cover_is_grey_and_dark() {
        let c = dimmed(hex(0x4A1D1C));
        assert!(c.r() < 0x30 && c.g() < 0x30 && c.b() < 0x30);
        assert!(c.r().abs_diff(c.b()) < 8);
    }

    #[test]
    fn high_contrast_is_black_and_white_with_strong_lines() {
        let p = Palette::new(ThemeKind::HighContrast, Accent::Ember);
        assert_eq!(p.t1, Color32::WHITE);
        assert_eq!(p.bg, Color32::BLACK);
        assert_eq!(p.accent_mark, Accent::Ember.ramp().a300);
        assert!(p.outlined());
        assert!(contrast(p.line, p.bg) >= MIN_NON_TEXT);
        assert_eq!(p.kind, ThemeKind::HighContrast);
    }

    #[test]
    fn accent_parse_round_trips() {
        for a in Accent::ALL {
            assert_eq!(Accent::parse(a.as_str()), Some(a));
        }
    }
}
