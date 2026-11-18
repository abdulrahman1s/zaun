//! The overlay's colours (`chat-calls.html` §G THEME): always the dark palette, since it sits
//! over unknown game pictures; high contrast swaps in opaque black plates. The accent follows
//! the user's (`easyplay_ui::theme::Accent`).
use easyplay_ui::theme::Accent;
use egui::Color32;

/// One palette, the overlay's tokens (the CSS custom properties of `.ovl`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub bg: Color32,
    pub raised: Color32,
    pub well: Color32,
    pub t1: Color32,
    pub t2: Color32,
    pub t3: Color32,
    pub t4: Color32,
    pub o1: Color32,
    pub o2: Color32,
    pub o3: Color32,
    pub line: Color32,
    pub line2: Color32,
    pub fill: Color32,
    pub fill_hover: Color32,
    pub on_fill: Color32,
    pub atext: Color32,
    pub amark: Color32,
    pub tint: Color32,
    pub amber: Color32,
    pub coral: Color32,
    pub on_coral: Color32,
    pub edge: Color32,
    /// The closed widgets' plate: the background at 78 % (opaque black in high contrast).
    pub plate: Color32,
    pub tip_bg: Color32,
    pub tip_fg: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const fn white(alpha: f32) -> Color32 {
    let a = (alpha * 255.0 + 0.5) as u8;
    Color32::from_rgba_premultiplied(a, a, a, a)
}

impl Look {
    pub fn new(accent: &str, high_contrast: bool) -> Self {
        let ramp = Accent::parse(accent).unwrap_or_default().ramp();
        let mut l = Self {
            bg: hex(0x0E0F10),
            raised: hex(0x1B1D1F),
            well: hex(0x151718),
            t1: hex(0xECEDEE),
            t2: hex(0xA7ABAF),
            t3: hex(0x888D92),
            t4: hex(0x4C5054),
            o1: white(0.05),
            o2: white(0.09),
            o3: white(0.14),
            line: white(0.09),
            line2: white(0.18),
            fill: ramp.a500,
            fill_hover: ramp.a400,
            on_fill: ramp.ink,
            atext: ramp.a300,
            amark: ramp.a500,
            tint: ramp.a500.gamma_multiply(0.16),
            amber: hex(0xF0B45A),
            coral: hex(0xFF7262),
            on_coral: hex(0x2B0A06),
            edge: white(0.07),
            plate: Color32::from_rgba_unmultiplied(14, 15, 16, 199),
            tip_bg: hex(0x2B2E31),
            tip_fg: hex(0xECEDEE),
        };
        if high_contrast {
            l.bg = Color32::BLACK;
            l.raised = hex(0x121212);
            l.t1 = Color32::WHITE;
            l.t2 = hex(0xEDEDED);
            l.t3 = hex(0xD2D2D2);
            l.line = white(0.5);
            l.amark = ramp.a300;
            l.plate = Color32::BLACK;
        }
        l
    }

    /// `c` at `alpha` of its own strength (fades, plates over the game).
    pub fn alpha(c: Color32, alpha: f32) -> Color32 {
        c.gamma_multiply(alpha.clamp(0.0, 1.0))
    }
}
