//! Icon font code points as named constants (UI_DESIGN_SYSTEM §6).
//!
//! The constants live in `icons_gen.rs`, generated with the font by
//! `assets/tools/build_fonts.py` from `assets/fonts/icons.txt`, so the list and the font
//! cannot diverge. A test checks that every name in the design system's table is present.

use egui::{FontFamily, FontId, RichText};

use super::typography::family;

#[path = "icons_gen.rs"]
mod generated;

pub use generated::*;

/// One icon: its semantic name and the code points of its two weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    /// Semantic name, e.g. `shield.offline`.
    pub name: &'static str,
    /// Regular weight (idle).
    pub regular: char,
    /// Fill weight (selected, active).
    pub fill: char,
    /// Listed in the design system's table (as opposed to a supporting glyph).
    pub documented: bool,
}

impl Icon {
    /// Used by the generated constants.
    pub const fn new(name: &'static str, regular: char, fill: char, documented: bool) -> Self {
        Self {
            name,
            regular,
            fill,
            documented,
        }
    }

    /// The glyph for a weight.
    pub const fn glyph(self, filled: bool) -> char {
        if filled { self.fill } else { self.regular }
    }

    /// The glyph as a string.
    pub fn text(self, filled: bool) -> String {
        self.glyph(filled).to_string()
    }

    /// Look an icon up by semantic name.
    pub fn by_name(name: &str) -> Option<Icon> {
        ALL.iter().copied().find(|i| i.name == name)
    }
}

/// The icon font at `size`.
pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(family::ICONS.into()))
}

/// `icon` at `size` as rich text (regular weight).
pub fn icon(icon: Icon, size: f32) -> RichText {
    RichText::new(icon.text(false)).font(font(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names in the first and third columns of the §6 table, expanding `a` / `.b` forms.
    fn documented_names() -> Vec<String> {
        let doc = include_str!("../../../../docs/UI_DESIGN_SYSTEM.md");
        let start = doc.find("## 6. Iconography").expect("section 6");
        let end = doc[start..].find("\nRules:").expect("end of table") + start;
        let mut names = Vec::new();
        for line in doc[start..end].lines().filter(|l| l.starts_with("| `")) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            for cell in [cells.get(1), cells.get(3)].into_iter().flatten() {
                let mut base = String::new();
                for part in cell.split('/') {
                    let n = part.trim().trim_matches('`');
                    if n.is_empty() {
                        continue;
                    }
                    if let Some(suffix) = n.strip_prefix('.') {
                        names.push(format!("{base}.{suffix}"));
                    } else {
                        base = n.split('.').next().unwrap_or(n).to_owned();
                        names.push(n.to_owned());
                    }
                }
            }
        }
        names
    }

    #[test]
    fn every_documented_icon_has_a_constant() {
        let names = documented_names();
        assert!(names.len() > 40, "parsed {names:?}");
        let missing: Vec<_> = names
            .iter()
            .filter(|n| Icon::by_name(n).is_none())
            .collect();
        assert!(missing.is_empty(), "missing constants: {missing:?}");
        let undocumented: Vec<_> = ALL
            .iter()
            .filter(|i| i.documented && !names.iter().any(|n| n == i.name))
            .map(|i| i.name)
            .collect();
        assert!(
            undocumented.is_empty(),
            "marked doc but not in §6: {undocumented:?}"
        );
    }

    #[test]
    fn code_points_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for i in ALL {
            assert!(seen.insert(i.regular), "{}", i.name);
            assert!(seen.insert(i.fill), "{}", i.name);
        }
    }

    #[test]
    fn every_icon_glyph_is_in_the_font() {
        let ctx = egui::Context::default();
        ctx.set_fonts(super::super::typography::font_definitions());
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        let f = font(20.0);
        let missing: Vec<&str> = ctx.fonts_mut(|fonts| {
            ALL.iter()
                .filter(|i| !fonts.has_glyph(&f, i.regular) || !fonts.has_glyph(&f, i.fill))
                .map(|i| i.name)
                .collect()
        });
        assert!(missing.is_empty(), "{missing:?}");
    }
}
