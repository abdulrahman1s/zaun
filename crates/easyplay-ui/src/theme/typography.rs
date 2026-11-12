//! Bundled fonts and the type scale (UI_DESIGN_SYSTEM §3, §9).
//!
//! Inter Variable is registered once per weight the scale uses, each with its `wght`
//! (and, for `display`, `opsz`) variation set through [`FontTweak`]. egui does not apply
//! OpenType features, so tabular figures come from `Inter-Tabular.ttf`, a digits-only face
//! with `tnum` baked in by `assets/tools/build_fonts.py`; the `numeric` families put it in
//! front of Inter. Scripts and emoji the bundled fonts lack fall back to system fonts through
//! eframe's `system_fonts` provider [F-311].

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::epaint::text::{FontInsert, InsertFontFamily, VariationCoords};
use egui::{FontData, FontDefinitions, FontFamily, FontId, FontTweak, RichText, TextStyle};

static INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Variable.ttf");
static INTER_TABULAR: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Tabular.ttf");
static MONO: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf");
static ICONS: &[u8] = include_bytes!("../../../../assets/fonts/easyplay-icons.ttf");

/// Font family names registered by [`font_definitions`].
pub mod family {
    /// The icon font alone.
    pub const ICONS: &str = "icons";
}

/// The weights the scale uses, with the `opsz` each is drawn at.
const WEIGHTS: [(u16, f32); 5] = [
    (400, 14.0),
    (450, 14.0),
    (550, 14.0),
    (600, 16.0),
    (650, 28.0),
];

fn inter_name(weight: u16) -> String {
    format!("inter-{weight}")
}

fn tabular_name(weight: u16) -> String {
    format!("inter-tnum-{weight}")
}

fn weight_family(weight: u16) -> FontFamily {
    FontFamily::Name(format!("w{weight}").into())
}

fn tabular_family(weight: u16) -> FontFamily {
    FontFamily::Name(format!("tnum{weight}").into())
}

fn tweak(weight: u16, opsz: f32) -> FontTweak {
    FontTweak {
        coords: VariationCoords::new([(b"wght", f32::from(weight)), (b"opsz", opsz)]),
        ..FontTweak::default()
    }
}

/// The complete font set: Inter at every weight of the scale, tabular digits, JetBrains Mono
/// and the icon font. Installed with `Context::set_fonts` before the first frame.
pub fn font_definitions() -> FontDefinitions {
    let mut font_data: BTreeMap<String, Arc<FontData>> = BTreeMap::new();
    let mut families: BTreeMap<FontFamily, Vec<String>> = BTreeMap::new();

    font_data.insert(
        family::ICONS.to_owned(),
        Arc::new(FontData::from_static(ICONS).tweak(FontTweak {
            // Phosphor sits on the baseline with a 960/64 split; nudge to optical centre.
            y_offset_factor: 0.0,
            ..FontTweak::default()
        })),
    );
    font_data.insert("mono".to_owned(), Arc::new(FontData::from_static(MONO)));
    for (w, opsz) in WEIGHTS {
        font_data.insert(
            inter_name(w),
            Arc::new(FontData::from_static(INTER).tweak(tweak(w, opsz))),
        );
        font_data.insert(
            tabular_name(w),
            Arc::new(FontData::from_static(INTER_TABULAR).tweak(tweak(w, opsz))),
        );
        families.insert(
            weight_family(w),
            vec![inter_name(w), family::ICONS.to_owned()],
        );
        families.insert(
            tabular_family(w),
            vec![tabular_name(w), inter_name(w), family::ICONS.to_owned()],
        );
    }
    families.insert(
        FontFamily::Proportional,
        vec![inter_name(400), family::ICONS.to_owned()],
    );
    families.insert(
        FontFamily::Monospace,
        vec!["mono".to_owned(), inter_name(400), family::ICONS.to_owned()],
    );
    families.insert(
        FontFamily::Name(family::ICONS.into()),
        vec![family::ICONS.to_owned()],
    );
    FontDefinitions {
        font_data,
        families,
    }
}

/// The bundled fonts as [`FontInsert`]s, for tests that need only one face.
pub fn icon_font_insert() -> FontInsert {
    FontInsert::new(
        family::ICONS,
        FontData::from_static(ICONS),
        vec![InsertFontFamily {
            family: FontFamily::Name(family::ICONS.into()),
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    )
}

/// A style token of the type scale (§3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// 28 / 34, 650: game title on the game page.
    Display,
    /// 20 / 26, 600: screen and sheet titles.
    Title,
    /// 16 / 22, 600: section headings.
    Heading,
    /// 14 / 20, 400: default text.
    Body,
    /// 14 / 20, 550: emphasis, button labels, card titles.
    BodyStrong,
    /// 12 / 16, 450: metadata, helper text.
    Caption,
    /// 11 / 14, 600, upper case with +4 % tracking: section labels, chips.
    Micro,
    /// 13 / 18, 400: technical text.
    Mono,
}

impl Type {
    /// Every token, largest first.
    pub const ALL: [Type; 8] = [
        Type::Display,
        Type::Title,
        Type::Heading,
        Type::Body,
        Type::BodyStrong,
        Type::Caption,
        Type::Micro,
        Type::Mono,
    ];

    /// The `TextStyle::Name` used for this token in `Style.text_styles`.
    pub const fn name(self) -> &'static str {
        match self {
            Type::Display => "display",
            Type::Title => "title",
            Type::Heading => "heading",
            Type::Body => "body",
            Type::BodyStrong => "body.strong",
            Type::Caption => "caption",
            Type::Micro => "micro",
            Type::Mono => "mono",
        }
    }

    /// Size in logical pixels.
    pub const fn size(self) -> f32 {
        match self {
            Type::Display => 28.0,
            Type::Title => 20.0,
            Type::Heading => 16.0,
            Type::Body | Type::BodyStrong => 14.0,
            Type::Caption => 12.0,
            Type::Micro => 11.0,
            Type::Mono => 13.0,
        }
    }

    /// Line height in logical pixels.
    pub const fn line_height(self) -> f32 {
        match self {
            Type::Display => 34.0,
            Type::Title => 26.0,
            Type::Heading => 22.0,
            Type::Body | Type::BodyStrong => 20.0,
            Type::Caption => 16.0,
            Type::Micro => 14.0,
            Type::Mono => 18.0,
        }
    }

    /// Weight (`wght` axis).
    pub const fn weight(self) -> u16 {
        match self {
            Type::Display => 650,
            Type::Title | Type::Heading | Type::Micro => 600,
            Type::Body | Type::Mono => 400,
            Type::BodyStrong => 550,
            Type::Caption => 450,
        }
    }

    /// The font for this token.
    pub fn font_id(self) -> FontId {
        let family = match self {
            Type::Mono => FontFamily::Monospace,
            Type::Body => FontFamily::Proportional,
            t => weight_family(t.weight()),
        };
        FontId::new(self.size(), family)
    }

    /// The same size and weight with tabular figures (the `numeric` token).
    pub fn numeric_font_id(self) -> FontId {
        FontId::new(self.size(), tabular_family(self.weight()))
    }

    /// The text style key.
    pub fn text_style(self) -> TextStyle {
        TextStyle::Name(self.name().into())
    }

    /// `text` in this style. `Micro` is upper-cased and tracked.
    pub fn rich(self, text: impl Into<String>) -> RichText {
        let text: String = text.into();
        match self {
            Type::Micro => RichText::new(text.to_uppercase())
                .font(self.font_id())
                .extra_letter_spacing(self.size() * 0.04),
            _ => RichText::new(text).font(self.font_id()),
        }
    }

    /// `text` in this style with tabular figures (numbers that change: elapsed time, bytes).
    pub fn numeric(self, text: impl Into<String>) -> RichText {
        RichText::new(text).font(self.numeric_font_id())
    }
}

/// `Style.text_styles`: the named tokens plus egui's built-in styles mapped onto them.
pub fn text_styles() -> BTreeMap<TextStyle, FontId> {
    let mut m: BTreeMap<TextStyle, FontId> = Type::ALL
        .iter()
        .map(|t| (t.text_style(), t.font_id()))
        .collect();
    m.insert(TextStyle::Body, Type::Body.font_id());
    m.insert(TextStyle::Button, Type::BodyStrong.font_id());
    m.insert(TextStyle::Heading, Type::Title.font_id());
    m.insert(TextStyle::Small, Type::Caption.font_id());
    m.insert(TextStyle::Monospace, Type::Mono.font_id());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_fonts() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(font_definitions());
        // Fonts are loaded at the start of a pass.
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        ctx
    }

    #[test]
    fn every_type_token_renders_latin_text_and_digits() {
        let ctx = ctx_with_fonts();
        for t in Type::ALL {
            let ok = ctx.fonts_mut(|f| {
                f.has_glyphs(&t.font_id(), "Hollow Knight 12 h · 3 days ago — ÄÖü")
                    && f.has_glyphs(&t.numeric_font_id(), "0123456789:")
            });
            assert!(ok, "{t:?}");
        }
    }

    #[test]
    fn tabular_digits_have_equal_advances() {
        let ctx = ctx_with_fonts();
        let font = Type::Display.numeric_font_id();
        let widths: Vec<f32> = ctx.fonts_mut(|f| {
            ('0'..='9')
                .map(|c| {
                    f.layout_no_wrap(c.to_string(), font.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                })
                .collect()
        });
        let first = widths[0];
        assert!(
            widths.iter().all(|w| (w - first).abs() < 0.01),
            "{widths:?}"
        );
        // Proportional Inter would make "1" narrower than "0".
        let prop: Vec<f32> = ctx.fonts_mut(|f| {
            ['0', '1']
                .iter()
                .map(|c| {
                    f.layout_no_wrap(c.to_string(), Type::Display.font_id(), egui::Color32::WHITE)
                        .size()
                        .x
                })
                .collect()
        });
        assert!(prop[1] < prop[0], "{prop:?}");
    }

    #[test]
    fn weights_change_glyph_widths() {
        let ctx = ctx_with_fonts();
        let w = |t: Type| {
            ctx.fonts_mut(|f| {
                f.layout_no_wrap(
                    "Library".into(),
                    FontId::new(14.0, t.font_id().family),
                    egui::Color32::WHITE,
                )
                .size()
                .x
            })
        };
        assert!(
            w(Type::Title) > w(Type::Body),
            "600 should be wider than 400"
        );
    }
}
