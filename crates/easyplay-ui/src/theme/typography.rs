//! Bundled fonts and the type scale (UI_DESIGN_SYSTEM §3, §9).
//!
//! Two faces carry the interface. The display face, Instrument Sans Condensed Bold, sets
//! posters, titles and headlines. Inter sets everything else at three weights; it is
//! registered once per weight, each with its `wght` variation set through [`FontTweak`].
//! egui does not apply OpenType features, so tabular figures come from digits-only faces
//! with `tnum` baked in by `assets/tools/build_fonts.py`; the `numeric` families put them in
//! front of their face. Scripts and emoji the bundled fonts lack fall back to system fonts
//! through eframe's `system_fonts` provider [F-311].

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::i18n::{self, Dir, bidi};
use egui::epaint::text::{FontInsert, InsertFontFamily, VariationCoords};
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Color32, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Galley, RichText, TextStyle,
    Ui,
};

static INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Variable.ttf");
static INTER_TABULAR: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Tabular.ttf");
static DISPLAY: &[u8] =
    include_bytes!("../../../../assets/fonts/InstrumentSans_Condensed-Bold.ttf");
static DISPLAY_TABULAR: &[u8] =
    include_bytes!("../../../../assets/fonts/InstrumentSans_Condensed-Bold-Tabular.ttf");
static ARABIC: &[u8] = include_bytes!("../../../../assets/fonts/NotoSansArabic-Variable.ttf");
static MONO: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf");
static ICONS: &[u8] = include_bytes!("../../../../assets/fonts/easyplay-icons.ttf");

/// Font family names registered by [`font_definitions`].
pub mod family {
    /// The icon font alone.
    pub const ICONS: &str = "icons";
    /// The display face: posters, titles, headlines.
    pub const DISPLAY: &str = "display";
    /// The display face with tabular figures.
    pub const DISPLAY_NUMERIC: &str = "display-tnum";
}

/// The weights of Inter the scale uses. Inter is drawn at its text optical size throughout:
/// the display face, not a display cut of Inter, sets everything large.
const WEIGHTS: [u16; 3] = [400, 500, 600];
const OPSZ: f32 = 14.0;

fn arabic_name(weight: u16) -> String {
    format!("arabic-{weight}")
}

/// The name of the Arabic face the display family uses (condensed, bold).
const ARABIC_DISPLAY: &str = "arabic-display";

/// The narrowest width the Arabic face's `wdth` axis allows and the display face wants.
const ARABIC_DISPLAY_WDTH: f32 = 75.0;

fn arabic_tweak(weight: f32, width: f32) -> FontTweak {
    FontTweak {
        coords: VariationCoords::new([(b"wght", weight), (b"wdth", width)]),
        ..FontTweak::default()
    }
}

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

fn tweak(weight: u16) -> FontTweak {
    FontTweak {
        coords: VariationCoords::new([(b"wght", f32::from(weight)), (b"opsz", OPSZ)]),
        ..FontTweak::default()
    }
}

/// The complete font set: the display face, Inter at every weight of the scale, tabular
/// digits for both, JetBrains Mono and the icon font. Installed with `Context::set_fonts`
/// before the first frame.
pub fn font_definitions() -> FontDefinitions {
    let mut font_data: BTreeMap<String, Arc<FontData>> = BTreeMap::new();
    let mut families: BTreeMap<FontFamily, Vec<String>> = BTreeMap::new();

    font_data.insert(
        family::ICONS.to_owned(),
        Arc::new(FontData::from_static(ICONS)),
    );
    font_data.insert("mono".to_owned(), Arc::new(FontData::from_static(MONO)));
    font_data.insert(
        family::DISPLAY.to_owned(),
        Arc::new(FontData::from_static(DISPLAY)),
    );
    font_data.insert(
        family::DISPLAY_NUMERIC.to_owned(),
        Arc::new(FontData::from_static(DISPLAY_TABULAR)),
    );
    // Noto Sans Arabic carries every Arabic-script character; it stands after the Latin faces
    // in each family, so it is never chosen for a space, a digit or a Latin letter, which
    // `i18n::bidi` relies on.
    font_data.insert(
        ARABIC_DISPLAY.to_owned(),
        Arc::new(FontData::from_static(ARABIC).tweak(arabic_tweak(700.0, ARABIC_DISPLAY_WDTH))),
    );
    for w in WEIGHTS {
        font_data.insert(
            arabic_name(w),
            Arc::new(FontData::from_static(ARABIC).tweak(arabic_tweak(f32::from(w), 100.0))),
        );
        font_data.insert(
            inter_name(w),
            Arc::new(FontData::from_static(INTER).tweak(tweak(w))),
        );
        font_data.insert(
            tabular_name(w),
            Arc::new(FontData::from_static(INTER_TABULAR).tweak(tweak(w))),
        );
        families.insert(
            weight_family(w),
            vec![inter_name(w), arabic_name(w), family::ICONS.to_owned()],
        );
        families.insert(
            tabular_family(w),
            vec![
                tabular_name(w),
                inter_name(w),
                arabic_name(w),
                family::ICONS.to_owned(),
            ],
        );
    }
    // A title in a script the display face lacks is set in Inter's heaviest weight here.
    families.insert(
        FontFamily::Name(family::DISPLAY.into()),
        vec![
            family::DISPLAY.to_owned(),
            inter_name(600),
            ARABIC_DISPLAY.to_owned(),
        ],
    );
    families.insert(
        FontFamily::Name(family::DISPLAY_NUMERIC.into()),
        vec![
            family::DISPLAY_NUMERIC.to_owned(),
            family::DISPLAY.to_owned(),
            inter_name(600),
            ARABIC_DISPLAY.to_owned(),
        ],
    );
    families.insert(
        FontFamily::Proportional,
        vec![inter_name(400), arabic_name(400), family::ICONS.to_owned()],
    );
    families.insert(
        FontFamily::Monospace,
        vec![
            "mono".to_owned(),
            inter_name(400),
            arabic_name(400),
            family::ICONS.to_owned(),
        ],
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

/// The display face at `size`: posters set every line at its own size.
pub fn display_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(family::DISPLAY.into()))
}

/// A style token of the type scale (§3). The first nine are set in the display face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// 56 / 54: the game title.
    Display,
    /// 44 / 43: the game title in a medium page.
    DisplayMedium,
    /// 36 / 36: the game title in a narrow page.
    DisplaySmall,
    /// 48 / 48: a page's headline sentence (Statistics, About).
    Hero,
    /// 44 / 44: the welcome title.
    Welcome,
    /// 36 / 37: the Activity headline; a headline sentence in a narrow page.
    Lede,
    /// 34 / 36: empty states.
    Blank,
    /// 34 / 34: the play record's number.
    Record,
    /// 26 / 28: dialog titles.
    Title,
    /// 15 / 22, 400: the sentence under a headline.
    Lead,
    /// 15 / 20, 600: section headings, Play.
    Heading,
    /// 15 / 20, 500: the Play pill when it is not the accent (Stop, Locate…).
    HeadingQuiet,
    /// 14 / 18, 500: the top bar's destinations.
    Nav,
    /// 14 / 20, 400: a dialog's text.
    Prose,
    /// 13 / 18, 400: default text, values.
    Body,
    /// 13 / 18, 500: labels that act.
    BodyStrong,
    /// 13 / 18, 600: primary buttons, a count that leads a line.
    BodyBold,
    /// 12 / 16, 400: help.
    Caption,
    /// 12 / 16, 500: table heads, tooltips.
    Label,
    /// 11 / 14, 400: counts, chart labels.
    Micro,
    /// 11 / 14, 500: a menu's section heading, keycaps.
    MicroStrong,
    /// 12 / 16, 400: file names, layers, logs.
    Mono,
}

impl Type {
    /// Every token, largest first.
    pub const ALL: [Type; 22] = [
        Type::Display,
        Type::Hero,
        Type::DisplayMedium,
        Type::Welcome,
        Type::DisplaySmall,
        Type::Lede,
        Type::Blank,
        Type::Record,
        Type::Title,
        Type::Lead,
        Type::Heading,
        Type::HeadingQuiet,
        Type::Nav,
        Type::Prose,
        Type::Body,
        Type::BodyStrong,
        Type::BodyBold,
        Type::Caption,
        Type::Label,
        Type::Micro,
        Type::MicroStrong,
        Type::Mono,
    ];

    /// The `TextStyle::Name` used for this token in `Style.text_styles`.
    pub const fn name(self) -> &'static str {
        match self {
            Type::Display => "display",
            Type::DisplayMedium => "display.medium",
            Type::DisplaySmall => "display.small",
            Type::Hero => "hero",
            Type::Welcome => "welcome",
            Type::Lede => "lede",
            Type::Blank => "blank",
            Type::Record => "record",
            Type::Title => "title",
            Type::Lead => "lead",
            Type::Heading => "heading",
            Type::HeadingQuiet => "heading.quiet",
            Type::Nav => "nav",
            Type::Prose => "prose",
            Type::Body => "body",
            Type::BodyStrong => "body.strong",
            Type::BodyBold => "body.bold",
            Type::Caption => "caption",
            Type::Label => "label",
            Type::Micro => "micro",
            Type::MicroStrong => "micro.strong",
            Type::Mono => "mono",
        }
    }

    /// Whether the token is set in the display face.
    pub const fn is_display(self) -> bool {
        matches!(
            self,
            Type::Display
                | Type::DisplayMedium
                | Type::DisplaySmall
                | Type::Hero
                | Type::Welcome
                | Type::Lede
                | Type::Blank
                | Type::Record
                | Type::Title
        )
    }

    /// Size in logical pixels.
    pub const fn size(self) -> f32 {
        match self {
            Type::Display => 56.0,
            Type::Hero => 48.0,
            Type::DisplayMedium | Type::Welcome => 44.0,
            Type::DisplaySmall | Type::Lede => 36.0,
            Type::Blank | Type::Record => 34.0,
            Type::Title => 26.0,
            Type::Lead | Type::Heading | Type::HeadingQuiet => 15.0,
            Type::Nav | Type::Prose => 14.0,
            Type::Body | Type::BodyStrong | Type::BodyBold => 13.0,
            Type::Caption | Type::Label | Type::Mono => 12.0,
            Type::Micro | Type::MicroStrong => 11.0,
        }
    }

    /// Line height in logical pixels.
    pub const fn line_height(self) -> f32 {
        match self {
            Type::Display => 54.0,
            Type::Hero => 48.0,
            Type::Welcome => 44.0,
            Type::DisplayMedium => 43.0,
            Type::Lede => 37.0,
            Type::DisplaySmall | Type::Blank => 36.0,
            Type::Record => 34.0,
            Type::Title => 28.0,
            Type::Lead => 22.0,
            Type::Heading | Type::HeadingQuiet | Type::Prose => 20.0,
            Type::Nav | Type::Body | Type::BodyStrong | Type::BodyBold => 18.0,
            Type::Caption | Type::Label | Type::Mono => 16.0,
            Type::Micro | Type::MicroStrong => 14.0,
        }
    }

    /// Weight (`wght` axis) of the tokens set in Inter; the display face has one weight.
    pub const fn weight(self) -> u16 {
        match self {
            Type::Lead | Type::Prose | Type::Body | Type::Caption | Type::Micro | Type::Mono => 400,
            Type::HeadingQuiet | Type::Nav | Type::BodyStrong | Type::Label | Type::MicroStrong => {
                500
            }
            _ => 600,
        }
    }

    /// Extra letter spacing in logical pixels: the game title is set a touch tighter.
    pub const fn tracking(self) -> f32 {
        match self {
            Type::Display | Type::DisplayMedium | Type::DisplaySmall => self.size() * -0.005,
            _ => 0.0,
        }
    }

    /// The font for this token.
    pub fn font_id(self) -> FontId {
        let family = match self {
            Type::Mono => FontFamily::Monospace,
            t if t.is_display() => FontFamily::Name(family::DISPLAY.into()),
            t if t.weight() == 400 => FontFamily::Proportional,
            t => weight_family(t.weight()),
        };
        FontId::new(self.size(), family)
    }

    /// The same size and weight with tabular figures (the `numeric` token).
    pub fn numeric_font_id(self) -> FontId {
        let family = if self.is_display() {
            FontFamily::Name(family::DISPLAY_NUMERIC.into())
        } else {
            tabular_family(self.weight())
        };
        FontId::new(self.size(), family)
    }

    /// The text style key.
    pub fn text_style(self) -> TextStyle {
        TextStyle::Name(self.name().into())
    }

    /// `text` in this style.
    pub fn rich(self, text: impl Into<String>) -> RichText {
        RichText::new(ordered(text.into()))
            .font(self.font_id())
            .extra_letter_spacing(self.tracking())
            .line_height(Some(self.line_height()))
    }

    /// `text` in this style with tabular figures (numbers that change: elapsed time, bytes).
    pub fn numeric(self, text: impl Into<String>) -> RichText {
        RichText::new(ordered(text.into()))
            .font(self.numeric_font_id())
            .extra_letter_spacing(self.tracking())
            .line_height(Some(self.line_height()))
    }

    /// The text format of this token: font, tracking and the scale's line height. Painted
    /// text is laid out with it.
    pub fn format(self, color: Color32, numeric: bool) -> TextFormat {
        TextFormat {
            font_id: if numeric {
                self.numeric_font_id()
            } else {
                self.font_id()
            },
            extra_letter_spacing: self.tracking(),
            line_height: Some(self.line_height()),
            color,
            ..TextFormat::default()
        }
    }

    /// A layout job for `text` wrapped at `width`, cut to `max_rows` with an ellipsis.
    pub fn job(self, text: &str, color: Color32, width: f32, max_rows: usize) -> LayoutJob {
        let mut job = LayoutJob::single_section(
            bidi::visual_line(text, base()).into_owned(),
            self.format(color, false),
        );
        job.wrap = TextWrapping {
            max_width: width.max(1.0),
            max_rows,
            // A single row is cut at the last letter that fits, several rows at words.
            break_anywhere: max_rows == 1,
            ..TextWrapping::default()
        };
        job
    }

    /// `text` laid out for painting: wrapped at `width`, at most `max_rows` rows. Goes
    /// through egui's layout cache, so a steady screen lays out no text.
    /// [`Galley::elided`] tells whether the full text needs a tooltip.
    pub fn galley(
        self,
        ui: &Ui,
        text: &str,
        color: Color32,
        width: f32,
        max_rows: usize,
    ) -> Arc<Galley> {
        if !bidi::has_rtl(text) {
            return ui.fonts_mut(|f| f.layout_job(self.job(text, color, width, max_rows)));
        }
        // No bidirectional layout in the engine: break the lines ourselves in logical order,
        // put each in visual order, and lay the result out with wrapping off.
        let format = self.format(color, false);
        ui.fonts_mut(|f| {
            let mut measure = |s: &str| {
                let job = LayoutJob::single_section(s.to_owned(), format.clone());
                f.layout_job(job).size().x
            };
            let (mut shaped, cut) =
                bidi::wrapped_cut(&mut measure, text, width.max(1.0), max_rows, base());
            if cut {
                // An empty row past the limit makes the engine report the text as elided
                // (callers show a tooltip); it draws nothing.
                shaped.push('\n');
            }
            let mut job = LayoutJob::single_section(shaped, format);
            job.wrap = TextWrapping {
                max_width: f32::INFINITY,
                max_rows,
                break_anywhere: false,
                overflow_character: None,
            };
            f.layout_job(job)
        })
    }

    /// Right-to-left `text` broken into lines of at most `width` and put in visual order,
    /// joined with newlines, for a caller that lays several runs out in one job; any other
    /// text comes back as it is.
    pub fn wrap_text<'a>(self, ui: &Ui, text: &'a str, width: f32) -> std::borrow::Cow<'a, str> {
        if !bidi::has_rtl(text) {
            return std::borrow::Cow::Borrowed(text);
        }
        let format = self.format(Color32::PLACEHOLDER, false);
        std::borrow::Cow::Owned(ui.fonts_mut(|f| {
            let mut measure = |s: &str| {
                f.layout_job(LayoutJob::single_section(s.to_owned(), format.clone()))
                    .size()
                    .x
            };
            bidi::wrapped(&mut measure, text, width.max(1.0), usize::MAX, base())
        }))
    }

    /// `text` as a wrapped paragraph of the surrounding layout: an ordinary wrapped label,
    /// except that right-to-left text is broken into lines and ordered by [`bidi`] first
    /// (egui's own wrapping has no bidirectional layout) and painted from a galley, with
    /// the logical text as the accessible label.
    pub fn paragraph(self, ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
        self.paragraph_with(ui, text, color, false)
    }

    /// [`Self::paragraph`] with tabular figures when `numeric`.
    pub fn paragraph_with(
        self,
        ui: &mut Ui,
        text: &str,
        color: Color32,
        numeric: bool,
    ) -> egui::Response {
        self.paragraph_lh(ui, text, color, numeric, None)
    }

    /// [`Self::paragraph_with`] on a line box `line_height` high instead of the token's.
    pub fn paragraph_lh(
        self,
        ui: &mut Ui,
        text: &str,
        color: Color32,
        numeric: bool,
        line_height: Option<f32>,
    ) -> egui::Response {
        if !bidi::has_rtl(text) {
            let mut format = self.format(color, numeric);
            if let Some(h) = line_height {
                format.line_height = Some(h);
            }
            let mut job = LayoutJob::single_section(text.to_owned(), format);
            job.wrap.max_width = ui.available_width();
            return ui.add(egui::Label::new(job).wrap());
        }
        let mut format = self.format(color, numeric);
        if let Some(h) = line_height {
            format.line_height = Some(h);
        }
        let width = ui.available_width();
        let g = ui.fonts_mut(|f| {
            let mut measure = |s: &str| {
                f.layout_job(LayoutJob::single_section(s.to_owned(), format.clone()))
                    .size()
                    .x
            };
            let shaped = bidi::wrapped(&mut measure, text, width.max(1.0), usize::MAX, base());
            f.layout_job(LayoutJob::single_section(shaped, format.clone()))
        });
        let (rect, resp) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
        ui.painter().galley(rect.min, g, color);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
        resp
    }

    /// One row of `text`, never wrapped.
    pub fn line(self, ui: &Ui, text: &str, color: Color32) -> Arc<Galley> {
        let job = LayoutJob::single_section(
            bidi::visual_line(text, base()).into_owned(),
            self.format(color, false),
        );
        ui.fonts_mut(|f| f.layout_job(job))
    }

    /// One row of `text` with tabular figures, never wrapped.
    pub fn numeric_galley(self, ui: &Ui, text: &str, color: Color32) -> Arc<Galley> {
        let job = LayoutJob::single_section(
            bidi::visual_line(text, base()).into_owned(),
            self.format(color, true),
        );
        ui.fonts_mut(|f| f.layout_job(job))
    }
}

/// The paragraph direction text is ordered for: right to left when the interface is,
/// otherwise taken from the text's first strong character.
pub(crate) fn base() -> Option<Dir> {
    (i18n::dir() == Dir::Rtl).then_some(Dir::Rtl)
}

/// `text` put in the order the engine must be given (see [`bidi`]), for widgets that build
/// their own text layouts.
pub(crate) fn shaped(text: &str) -> std::borrow::Cow<'_, str> {
    bidi::visual_line(text, base())
}

/// `text` put in the order the engine must be given (see [`bidi`]).
fn ordered(text: String) -> String {
    match bidi::visual_line(&text, base()) {
        std::borrow::Cow::Borrowed(_) => text,
        std::borrow::Cow::Owned(o) => o,
    }
}

/// A bare context with `defs` installed and its first pass run, for tests.
#[cfg(test)]
pub(crate) fn test_context(defs: FontDefinitions) -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(defs);
    // Fonts are loaded at the start of a pass; the texture upload is discarded.
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    ctx
}

/// `Style.text_styles`: the named tokens plus egui's built-in styles mapped onto them.
pub fn text_styles() -> BTreeMap<TextStyle, FontId> {
    let mut m: BTreeMap<TextStyle, FontId> = Type::ALL
        .iter()
        .map(|t| (t.text_style(), t.font_id()))
        .collect();
    m.insert(TextStyle::Body, Type::Body.font_id());
    m.insert(TextStyle::Button, Type::BodyStrong.font_id());
    m.insert(TextStyle::Heading, Type::Heading.font_id());
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
                f.has_glyphs(&t.font_id(), "Hollow Knight 12 h, 3 days ago: ÄÖü & ’")
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
                    f.layout_no_wrap(c.to_string(), font.clone(), Color32::WHITE)
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
                    f.layout_no_wrap(c.to_string(), Type::Display.font_id(), Color32::WHITE)
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
                    Color32::WHITE,
                )
                .size()
                .x
            })
        };
        assert!(
            w(Type::Heading) > w(Type::Body),
            "600 should be wider than 400"
        );
        assert!(
            w(Type::Title) < w(Type::Body),
            "the display face is condensed"
        );
    }

    #[test]
    fn every_type_token_has_arabic_glyphs() {
        let ctx = ctx_with_fonts();
        for t in Type::ALL {
            let ok = ctx.fonts_mut(|f| {
                f.has_glyphs(&t.font_id(), "إضافة لعبة ؟،")
                    && f.has_glyphs(&t.numeric_font_id(), "إضافة لعبة ؟،")
            });
            assert!(ok, "{t:?}");
        }
    }

    /// `i18n::bidi` assumes Arabic letters come from the Arabic face and nothing else does:
    /// the Latin faces have no Arabic letter, and in every family the faces before the
    /// first Arabic one already cover a space, the digits and "A".
    #[test]
    fn the_arabic_face_draws_arabic_and_only_arabic() {
        let defs = font_definitions();
        let ctx = ctx_with_fonts();
        let arabic_in = |name: &str| name.starts_with("arabic-");
        for ch in ['\u{0627}', '\u{0628}', '\u{060C}'] {
            for name in ["inter-400", "inter-600", "mono", family::DISPLAY] {
                let mut d = font_definitions();
                d.families
                    .insert(FontFamily::Name("probe".into()), vec![name.to_owned()]);
                let ctx = test_context(d);
                let has = ctx.fonts_mut(|f| {
                    f.has_glyphs(
                        &FontId::new(12.0, FontFamily::Name("probe".into())),
                        &ch.to_string(),
                    )
                });
                assert!(!has, "{name} has U+{:04X}", ch as u32);
            }
        }
        for (fam, names) in &defs.families {
            let Some(first) = names.iter().position(|n| arabic_in(n)) else {
                continue;
            };
            assert!(names[..first].iter().all(|n| !arabic_in(n)));
            let probe = FontFamily::Name(format!("prefix-{fam:?}").into());
            let mut d = font_definitions();
            d.families.insert(probe.clone(), names[..first].to_vec());
            let c = test_context(d);
            let ok = c.fonts_mut(|f| f.has_glyphs(&FontId::new(12.0, probe), " 0123456789A"));
            assert!(ok, "{fam:?}");
        }
        // And the Arabic face really is reached for Arabic letters in every token.
        for t in Type::ALL {
            assert!(
                ctx.fonts_mut(|f| f.has_glyphs(&t.font_id(), "ابد")),
                "{t:?}"
            );
        }
    }

    #[test]
    fn rtl_galleys_wrap_in_reading_order_and_report_elision() {
        let ctx = ctx_with_fonts();
        let text = "يحتفظ إيزي بلاي بكل لعبة في بيئة معزولة خاصة بها، فلا تستطيع لعبة قراءة ملفات لعبة أخرى.";
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let g = Type::Body.galley(ui, text, Color32::WHITE, 200.0, 10);
            assert!(g.rows.len() > 1 && !g.elided);
            assert!(g.size().x <= 200.5, "{}", g.size().x);
            let cut = Type::Body.galley(ui, text, Color32::WHITE, 200.0, 1);
            assert_eq!(cut.rows.len(), 1);
            assert!(cut.elided);
            assert!(cut.size().x <= 200.5);
            let plain = Type::Body.galley(ui, "Hello", Color32::WHITE, 200.0, 1);
            assert!(!plain.elided);
        });
        out.textures_delta.clear();
    }
}
