//! Deterministic generated covers (UI_DESIGN_SYSTEM §7.2, ADR-0009).
//!
//! [`placeholder_for`] derives the parameters from the game's name alone (SHA-256 of the
//! normalised name, so the same name gives the same placeholder on every run and machine).
//! The UI paints them (gradient, disc, initials in its own font); [`render`] produces the
//! same gradient and disc as pixels for shortcut icons and exports. The exported image has
//! no text: the core has no font rasteriser.

use sha2::{Digest, Sha256};

use super::codec::{CodecError, ImageCodec, RgbaImage};

/// Parameters of a generated cover.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Placeholder {
    /// One or two upper-case initials ("HK" for "Hollow Knight"); may be empty.
    pub initials: String,
    /// Gradient start (top), sRGB.
    pub top: [u8; 3],
    /// Gradient end (bottom), sRGB.
    pub bottom: [u8; 3],
    /// Disc colour, sRGB.
    pub accent: [u8; 3],
    /// Disc centre as a fraction of width and height, in 1/100.
    pub disc_centre: (u8, u8),
    /// Disc radius as a fraction of width, in 1/100.
    pub disc_radius: u8,
    /// Seed for any further decoration the UI wants.
    pub seed: u64,
}

impl Placeholder {
    /// Average of the gradient: the "dominant colour" of a generated cover.
    pub fn dominant(&self) -> [u8; 3] {
        mix(self.top, self.bottom, 0.5)
    }
}

fn initials(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .take(2)
        .collect()
}

/// HSL (all in 0..1) to sRGB.
fn hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let h = h.rem_euclid(1.0);
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let ch = |t: f32| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [ch(h + 1.0 / 3.0), ch(h), ch(h - 1.0 / 3.0)]
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// The placeholder of a game called `name`.
pub fn placeholder_for(name: &str) -> Placeholder {
    let norm: String = name.trim().to_lowercase();
    let d: [u8; 32] = Sha256::digest(norm.as_bytes()).into();
    let hue = f32::from(u16::from_le_bytes([d[0], d[1]])) / 65_536.0;
    let hue2 = hue + 0.08 + f32::from(d[2] % 64) / 640.0;
    let mut seed = [0u8; 8];
    seed.copy_from_slice(&d[24..32]);
    Placeholder {
        initials: initials(name),
        top: hsl(hue, 0.45, 0.36),
        bottom: hsl(hue2, 0.55, 0.14),
        accent: hsl(hue + 0.5, 0.6, 0.62),
        disc_centre: (30 + d[3] % 40, 30 + d[4] % 20),
        disc_radius: 14 + d[5] % 12,
        seed: u64::from_le_bytes(seed),
    }
}

/// Paint `p` (gradient and a soft disc, no text) at `w × h`.
pub fn render(p: &Placeholder, w: u32, h: u32) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    let (wf, hf) = (w as f32, h as f32);
    let cx = wf * f32::from(p.disc_centre.0) / 100.0;
    let cy = hf * f32::from(p.disc_centre.1) / 100.0;
    let r = wf * f32::from(p.disc_radius) / 100.0;
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        let base = mix(p.top, p.bottom, y as f32 / hf);
        for x in 0..w {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let c = if d < r {
                mix(p.accent, base, (d / r).powi(6))
            } else if d < r * 1.8 {
                mix(base, p.accent, 0.12 * (1.0 - (d - r) / (r * 0.8)))
            } else {
                base
            };
            pixels.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    RgbaImage {
        width: w,
        height: h,
        pixels,
    }
}

/// [`render`] encoded as PNG (shortcut icons, exports).
pub fn render_png(
    codec: &dyn ImageCodec,
    p: &Placeholder,
    w: u32,
    h: u32,
) -> Result<Vec<u8>, CodecError> {
    codec.encode_png(&render(p, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artwork::ImageCrateCodec;

    /// Acceptance (ART-005): same name → same placeholder across runs. The expected values
    /// are pinned so a change of the derivation is noticed.
    #[test]
    fn deterministic_and_pinned() {
        let a = placeholder_for("Hollow Knight");
        assert_eq!(a, placeholder_for("Hollow Knight"));
        assert_eq!(a, placeholder_for("  hollow knight "));
        assert_ne!(a, placeholder_for("Celeste"));
        assert_eq!(a.initials, "HK");
        let png1 = render_png(&ImageCrateCodec, &a, 60, 90).unwrap();
        let png2 = render_png(&ImageCrateCodec, &placeholder_for("Hollow Knight"), 60, 90).unwrap();
        assert_eq!(png1, png2);
        let pinned = format!(
            "{:?}",
            (a.top, a.bottom, a.accent, a.disc_centre, a.disc_radius)
        );
        insta::assert_snapshot!(pinned);
    }

    #[test]
    fn initials_cases() {
        assert_eq!(initials("Celeste"), "C");
        assert_eq!(initials("The Witcher 3: Wild Hunt"), "TW");
        assert_eq!(initials("  "), "");
        assert_eq!(initials("ßtraße x"), "SS");
        assert_eq!(initials("2048"), "2");
    }
}
