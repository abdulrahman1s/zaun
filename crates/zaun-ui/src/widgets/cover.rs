//! Covers (UI_DESIGN_SYSTEM §7.2). Real artwork is an image. Without it Zaun sets the
//! title as a poster: one or two words to a line, every line scaled to fill the width,
//! stacked from the bottom on one of ten paper-and-ink pairs. A game keeps its colours for
//! as long as it keeps its name, so a wall of posters reads as one designed series.
//!
//! Every line of every poster has its own size, and covers stretch with the window. Laid
//! out at their exact sizes the glyph atlas would grow without bound, so a line is laid out
//! at the next of a few fixed sizes and its mesh is scaled down to fit (by at most a ninth).

use std::collections::HashMap;
use std::sync::Arc;

use egui::emath::TSTransform;
use egui::epaint::RectShape;
use egui::mutex::Mutex;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align2, Color32, Context, CornerRadius, Id, Painter, Pos2, Rect, Shape, Stroke, StrokeKind,
    TextureId, pos2, vec2,
};

use super::brand;
use crate::theme::Theme;
use crate::theme::palette::{self, Paper, Shade, dimmed, fade, mix, over};
use crate::theme::tokens::{alpha, layout, poster as measure, radius, stroke};
use crate::theme::typography::display_font;

/// Words that ride with a neighbour on a poster and carry no initial.
const SMALL: [&str; 13] = [
    "a", "an", "and", "at", "by", "for", "in", "of", "on", "or", "the", "to", "&",
];

/// The size, in points, at which a line is measured.
const MEASURE_AT: f32 = 100.0;

fn is_small(word: &str) -> bool {
    SMALL.contains(&word)
}

/// A word as the poster compares it: lower case, without `:`, `,` and `.`.
fn bare(word: &str) -> String {
    word.to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ':' | ',' | '.'))
        .collect()
}

/// A Roman numeral up to ten, or one or two digits ("II", "3").
fn is_numeral(bare: &str) -> bool {
    matches!(
        bare,
        "i" | "ii" | "iii" | "iv" | "v" | "vi" | "vii" | "viii" | "ix" | "x"
    ) || ((1..=2).contains(&bare.len()) && bare.bytes().all(|b| b.is_ascii_digit()))
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The lines of the poster for `title`: the title split on spaces; a small word joins the
/// word after it; a trailing small word or a numeral joins the word before it; more than six
/// lines merge the adjacent pair with the fewest letters until six are left.
pub fn poster_lines(title: &str) -> Vec<String> {
    let words: Vec<&str> = title.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let b = bare(w);
        if is_small(&b) && i + 1 < words.len() {
            lines.push(format!("{w} {}", words[i + 1]));
            i += 1;
        } else if (is_small(&b) || is_numeral(&b)) && !lines.is_empty() {
            if let Some(last) = lines.last_mut() {
                last.push(' ');
                last.push_str(w);
            }
        } else {
            lines.push(w.to_owned());
        }
        i += 1;
    }
    while lines.len() > measure::MAX_LINES {
        let mut at = 0;
        let mut min = usize::MAX;
        for k in 0..lines.len() - 1 {
            let n = units(&lines[k]) + units(&lines[k + 1]);
            if n < min {
                min = n;
                at = k;
            }
        }
        let next = lines.remove(at + 1);
        lines[at].push(' ');
        lines[at].push_str(&next);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Up to two initials for a cover too small for a poster: the first letters of the first
/// two words that are not small words ("Moss & Iron" → "MI"); of the title itself when
/// every word is small.
pub fn initials(title: &str) -> String {
    let out: String = title
        .split_whitespace()
        .filter(|w| !is_small(&w.to_lowercase()))
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    if !out.is_empty() {
        return out;
    }
    title
        .trim()
        .chars()
        .next()
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

/// A title set as a poster. Sizes are fractions of the cover's width.
#[derive(Clone, Debug, PartialEq)]
pub struct Poster {
    /// Each line and its font size.
    pub lines: Vec<(String, f32)>,
    /// Extra inset from the bottom edge, for a last line with letters that hang below it.
    pub raise: f32,
}

/// Two decimals of a percentage: the precision the sizes are kept at.
fn hundredths(percent: f32) -> f32 {
    (percent * 100.0).round() / 100.0
}

/// Fit `lines` given each line's width at size 1. Works in percentages of the cover's
/// width, as the design does.
fn fit(lines: Vec<String>, width_at_1: &[f32]) -> Poster {
    let fill = measure::FILL * 100.0;
    let max = measure::MAX_SIZE * 100.0;
    let mut sizes: Vec<f32> = width_at_1
        .iter()
        .map(|w| {
            if *w > 0.0 {
                (fill / w * measure::EASE).min(max)
            } else {
                max
            }
        })
        .collect();
    let hangs = lines
        .last()
        .is_some_and(|l| l.chars().any(|c| "gjpqy,;".contains(c)));
    let hang = |sizes: &[f32]| {
        if hangs {
            (sizes.last().copied().unwrap_or(0.0) * measure::HANG - measure::HANG_LESS * 100.0)
                .max(0.0)
        } else {
            0.0
        }
    };
    let tall: f32 = sizes.iter().map(|s| s * measure::LINE).sum();
    let room = measure::ROOM * 100.0 - hang(&sizes);
    if tall > room {
        for s in &mut sizes {
            *s *= room / tall;
        }
    }
    let raise = hundredths(hang(&sizes)) / 100.0;
    Poster {
        lines: lines
            .into_iter()
            .zip(sizes)
            .map(|(t, s)| (t, hundredths(s) / 100.0))
            .collect(),
        raise,
    }
}

/// The width of `line` in the display face at size 1.
fn width_at_1(ctx: &Context, line: &str) -> f32 {
    let mut job = LayoutJob::single_section(
        line.to_owned(),
        TextFormat {
            font_id: display_font(MEASURE_AT),
            ..TextFormat::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    job.round_output_to_gui = false;
    ctx.fonts_mut(|f| f.layout_job(job)).size().x / MEASURE_AT
}

#[derive(Default)]
struct PosterCache {
    pixels_per_point: f32,
    posters: HashMap<String, Arc<Poster>>,
}

/// The poster for `title`, measured once and kept.
pub fn poster(ctx: &Context, title: &str) -> Arc<Poster> {
    let cache = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<PosterCache>>>(Id::unique("zaun.posters"))
            .clone()
    });
    let ppp = ctx.pixels_per_point();
    {
        let mut c = cache.lock();
        if c.pixels_per_point != ppp {
            c.pixels_per_point = ppp;
            c.posters.clear();
        }
        if let Some(p) = c.posters.get(title) {
            return p.clone();
        }
    }
    let lines = poster_lines(title);
    let widths: Vec<f32> = lines.iter().map(|l| width_at_1(ctx, l)).collect();
    let made = Arc::new(fit(lines, &widths));
    cache.lock().posters.insert(title.to_owned(), made.clone());
    made
}

/// The size a line wanted at `size` is laid out at: the next of a ladder of sizes a sixth of
/// an octave apart, so that a wall of posters of any width shares a bounded set of glyphs.
fn layout_size(size: f32) -> f32 {
    const BASE: f32 = 8.0;
    const STEPS_PER_OCTAVE: f32 = 6.0;
    let n = ((size.max(BASE) / BASE).log2() * STEPS_PER_OCTAVE - 1e-4)
        .ceil()
        .max(0.0);
    BASE * 2.0_f32.powf(n / STEPS_PER_OCTAVE)
}

/// One line of a poster: its line box's top left at `pos`, set at `size`.
fn paint_line(painter: &Painter, pos: Pos2, text: &str, size: f32, ink: Color32) {
    if text.is_empty() || size <= 0.0 {
        return;
    }
    let at = layout_size(size);
    let ordered = crate::i18n::bidi::visual_line(text, crate::theme::typography::base());
    let mut job = LayoutJob::single_section(
        ordered.into_owned(),
        TextFormat {
            font_id: display_font(at),
            extra_letter_spacing: measure::TRACKING * at,
            line_height: Some(measure::LINE * at),
            color: ink,
            ..TextFormat::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    job.round_output_to_gui = false;
    let galley = painter.ctx().fonts_mut(|f| f.layout_job(job));
    let mut shape = Shape::galley(Pos2::ZERO, galley, ink);
    shape.transform(TSTransform::new(pos.to_vec2(), size / at));
    painter.add(shape);
}

/// The corner of a cover occupying `rect`.
pub fn corner(rect: Rect) -> CornerRadius {
    CornerRadius::same(radius::cover(rect.width()))
}

/// What to paint as the cover.
#[derive(Clone, Copy, Debug)]
pub enum CoverSource {
    /// A loaded texture; `fade` 0–1 for the fade-in.
    Texture(TextureId, f32),
    /// Texture pending: dominant colour fill.
    Pending(Color32),
    /// No artwork: the poster.
    Placeholder,
}

/// How a cover is drawn besides its source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoverLook {
    /// The game's program cannot be found: the cover is drawn grey and dark.
    pub missing: bool,
    /// Show the mark in the poster's corner (hidden while a selection check sits there).
    pub mark: bool,
    /// Opacity of the whole cover (a cover fading in).
    pub opacity: f32,
}

impl Default for CoverLook {
    fn default() -> Self {
        Self {
            missing: false,
            mark: true,
            opacity: 1.0,
        }
    }
}

/// Paint the poster for `title` into `rect`.
fn paint_poster(painter: &Painter, rect: Rect, title: &str, look: CoverLook, theme: &Theme) {
    let w = rect.width();
    let corner = corner(rect);
    let Paper { paper, ink } = palette::paper_for(title);
    let (paper, ink) = if look.missing {
        (dimmed(paper), dimmed(ink))
    } else {
        (paper, ink)
    };
    let f = |c: Color32| fade(c, look.opacity);
    painter.rect_filled(rect, corner, f(paper));
    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    if w < layout::POSTER_MIN_W {
        clipped.text(
            rect.center(),
            Align2::CENTER_CENTER,
            initials(title),
            display_font(w * measure::INITIALS),
            f(ink),
        );
    } else {
        if look.mark {
            let side = w * measure::MARK;
            let at = rect.min + vec2(w * measure::INSET_X, w * measure::INSET_X);
            brand::paint_mark(
                &clipped,
                Rect::from_min_size(at, vec2(side, side)),
                f(mix(paper, ink, alpha::POSTER_MARK)),
                f(paper),
            );
        }
        let set = poster(painter.ctx(), title);
        let left = rect.left() + w * measure::INSET_X;
        let mut y = rect.bottom() - w * (measure::INSET_BOTTOM + set.raise);
        for (text, size) in set.lines.iter().rev() {
            let size = size * w;
            y -= size * measure::LINE;
            paint_line(&clipped, pos2(left, y), text, size, f(ink));
        }
    }
    let edge = if look.missing {
        // The whole cover is dimmed, its inner line with it.
        let lit = over(theme.palette.edge, palette::paper_for(title).paper);
        mix(paper, dimmed(lit), 1.0)
    } else {
        theme.palette.edge
    };
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(stroke::BORDER, f(edge)),
        StrokeKind::Inside,
    );
}

/// Paint a cover into `rect`: the artwork when there is some, otherwise the poster for
/// `title`; then the inner line every cover has.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    source: CoverSource,
    title: &str,
    look: CoverLook,
    theme: &Theme,
) {
    let corner = corner(rect);
    let f = |c: Color32| fade(c, look.opacity);
    match source {
        CoverSource::Placeholder => {
            paint_poster(painter, rect, title, look, theme);
            return;
        }
        CoverSource::Pending(c) => {
            let c = if look.missing { dimmed(c) } else { c };
            painter.rect_filled(rect, corner, f(c));
        }
        CoverSource::Texture(id, t) => {
            if t < 1.0 {
                painter.rect_filled(rect, corner, f(Color32::BLACK));
            }
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            // A missing game's artwork at half its brightness, under a grey veil that takes
            // most of its colour.
            let tint = if look.missing {
                palette::fixed::MISSING_ART_TINT
            } else {
                Color32::WHITE
            };
            painter.add(
                RectShape::filled(rect, corner, fade(tint, t * look.opacity)).with_texture(id, uv),
            );
            if look.missing {
                painter.rect_filled(
                    rect,
                    corner,
                    fade(palette::fixed::MISSING_ART_VEIL, 0.7 * t * look.opacity),
                );
            }
        }
    }
    painter.rect_stroke(
        rect,
        corner,
        Stroke::new(stroke::BORDER, f(theme.palette.edge)),
        StrokeKind::Inside,
    );
}

/// The largest Steam badge glyph, in points (a wall cover, the game page).
pub const STEAM_BADGE_MAX: f32 = 16.0;
/// The smallest (a list thumbnail).
pub const STEAM_BADGE_MIN: f32 = 9.0;
/// The badge glyph's size as a fraction of the cover's width, between the two.
pub const STEAM_BADGE_SHARE: f32 = 0.1;
/// Opacity of the glyph on artwork.
pub const STEAM_BADGE_INK: f32 = 0.7;
/// Opacity of the disc under it on artwork.
pub const STEAM_BADGE_DISC: f32 = 0.6;

/// Where the Steam badge of a cover occupying `rect` sits, and its glyph size: the top-left
/// corner, where an Zaun poster has its mark (a game Steam runs has none, ADR-0031), at
/// the inset of the cover's other badges; away from the poster's title (the bottom) and
/// from Play and the favourite (the top right).
pub fn steam_badge_at(rect: Rect) -> (Pos2, f32) {
    let w = rect.width();
    let size = (w * STEAM_BADGE_SHARE).clamp(STEAM_BADGE_MIN, STEAM_BADGE_MAX);
    let radius = size * 0.62;
    let inset = if w >= layout::POSTER_MIN_W {
        crate::theme::tokens::control::DISC_INSET
    } else {
        (w * measure::INSET_X).max(1.5)
    };
    (rect.min + vec2(inset + radius, inset + radius), size)
}

/// Steam's logo on the cover of a game Steam runs (ADR-0031): small and quiet. On artwork,
/// [`Palette::on_art`](crate::theme::palette::Palette) over a translucent disc of the
/// favourite's colour; on a poster, in the colour of the poster's own mark. Callers leave it
/// out while the selection check takes that corner.
pub fn paint_steam_badge(
    painter: &Painter,
    rect: Rect,
    source: CoverSource,
    title: &str,
    look: CoverLook,
    theme: &Theme,
) {
    let (c, size) = steam_badge_at(rect);
    let p = &theme.palette;
    let f = |col: Color32, a: f32| fade(col, a * look.opacity);
    let glyph = crate::theme::icons::STEAM.text(false);
    let font = crate::theme::icons::font(size);
    let w = rect.width();
    match source {
        // Exactly where, and as large as, an Zaun poster's mark: the posters of a wall
        // keep one line.
        CoverSource::Placeholder if w >= layout::POSTER_MIN_W => {
            let Paper { paper, ink } = palette::paper_for(title);
            let (paper, ink) = if look.missing {
                (dimmed(paper), dimmed(ink))
            } else {
                (paper, ink)
            };
            let side = w * measure::MARK;
            let at = rect.min + vec2(w * measure::INSET_X, w * measure::INSET_X);
            painter.text(
                at + vec2(side / 2.0, side / 2.0),
                Align2::CENTER_CENTER,
                glyph,
                crate::theme::icons::font(side.min(STEAM_BADGE_MAX)),
                f(mix(paper, ink, alpha::POSTER_MARK), 1.0),
            );
        }
        CoverSource::Placeholder => {
            let Paper { paper, ink } = palette::paper_for(title);
            let (paper, ink) = if look.missing {
                (dimmed(paper), dimmed(ink))
            } else {
                (paper, ink)
            };
            painter.text(
                c,
                Align2::CENTER_CENTER,
                glyph,
                font,
                f(mix(paper, ink, alpha::POSTER_MARK), 1.0),
            );
        }
        CoverSource::Pending(_) | CoverSource::Texture(..) => {
            let t = match source {
                CoverSource::Texture(_, t) => t,
                _ => 1.0,
            };
            painter.circle_filled(c, size * 0.62, f(p.badge, STEAM_BADGE_DISC * t));
            painter.text(
                c,
                Align2::CENTER_CENTER,
                glyph,
                font,
                f(p.on_art, STEAM_BADGE_INK * t),
            );
        }
    }
}

/// A drop shadow under a cover occupying `rect` (the lift under the pointer, the cover of
/// the game page), at `opacity`.
pub fn paint_shadow(painter: &Painter, rect: Rect, shade: Shade, opacity: f32) {
    if opacity <= 0.0 {
        return;
    }
    let mut shadow = shade.shadow();
    shadow.color = fade(shadow.color, opacity);
    painter.add(shadow.as_shape(rect, corner(rect)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(title: &str) -> Vec<String> {
        poster_lines(title)
    }

    /// The splitting rules, against the design's own results.
    #[test]
    fn titles_split_as_the_design_splits_them() {
        assert_eq!(lines("Lantern Keep"), ["Lantern", "Keep"]);
        assert_eq!(lines("Low Tide Motel"), ["Low", "Tide", "Motel"]);
        // A small word rides with the word after it.
        assert_eq!(lines("Moss & Iron"), ["Moss", "& Iron"]);
        assert_eq!(
            lines("The Long Winter Road: Director's Edition"),
            ["The Long", "Winter", "Road:", "Director's", "Edition"]
        );
        assert_eq!(
            lines("The Legend of Zelda: Breath of the Wild"),
            ["The Legend", "of Zelda:", "Breath", "of the", "Wild"]
        );
        assert_eq!(
            lines("Ori and the Blind Forest"),
            ["Ori", "and the", "Blind", "Forest"]
        );
        // A numeral joins the word before it, with or without a colon.
        assert_eq!(lines("Amber Circuit II"), ["Amber", "Circuit II"]);
        assert_eq!(
            lines("Half-Life 2: Episode Two"),
            ["Half-Life 2:", "Episode", "Two"]
        );
        assert_eq!(lines("Baldur’s Gate 3"), ["Baldur’s", "Gate 3"]);
        assert_eq!(
            lines("Sid Meier’s Civilization VI"),
            ["Sid", "Meier’s", "Civilization VI"]
        );
        assert_eq!(
            lines("Tom Clancy’s The Division 2"),
            ["Tom", "Clancy’s", "The Division 2"]
        );
        assert_eq!(
            lines("A Plague Tale: Requiem"),
            ["A Plague", "Tale:", "Requiem"]
        );
        assert_eq!(lines("Q"), ["Q"]);
        assert_eq!(lines("  "), [""]);
    }

    #[test]
    fn more_than_six_lines_merge_the_shortest_neighbours() {
        let l = lines("Alpha Beta Gamma Delta Epsilon Zeta Eta Theta");
        assert_eq!(l.len(), 6);
        assert_eq!(l.join(" "), "Alpha Beta Gamma Delta Epsilon Zeta Eta Theta");
        // "Zeta Eta" is the shortest pair, then "Zeta Eta" with "Theta" is not: "Alpha Beta".
        assert!(l.contains(&"Zeta Eta".to_owned()), "{l:?}");
    }

    #[test]
    fn initials_of_titles() {
        assert_eq!(initials("Lantern Keep"), "LK");
        assert_eq!(initials("Moss & Iron"), "MI");
        assert_eq!(initials("The Long Winter Road: Director's Edition"), "LW");
        assert_eq!(initials("Half-Life 2: Episode Two"), "H2");
        assert_eq!(initials("Tidewater"), "T");
        assert_eq!(initials("celeste"), "C");
        assert_eq!(initials("The Of"), "T");
        assert_eq!(initials("  "), "?");
    }

    /// The fitting arithmetic, with the widths the design measured (per 100 units of size).
    #[test]
    fn lines_are_fitted_as_the_design_fits_them() {
        let same = |a: f32, b: f32| assert!((a - b).abs() < 1e-5, "{a} vs {b}");
        let p = fit(
            vec!["Lantern".into(), "Keep".into()],
            &[2.758_997, 1.833_998],
        );
        same(p.lines[0].1, 0.3070);
        same(p.lines[1].1, 0.4619);
        same(p.raise, 0.0354);
        // A short word is capped at 58 % of the width.
        let p = fit(vec!["Amber".into(), "Sky".into()], &[2.403_5, 1.312_998_8]);
        same(p.lines[1].1, 0.58);
        same(p.raise, 0.0496);
        // No letter hangs below "Tactics": nothing is raised.
        let p = fit(
            vec!["Starfall".into(), "Tactics".into()],
            &[2.654_997_5, 2.518_997_5],
        );
        same(p.lines[0].1, 0.3191);
        same(p.lines[1].1, 0.3363);
        same(p.raise, 0.0);
    }

    /// A stack too tall for the cover shrinks every line alike.
    #[test]
    fn a_tall_stack_is_shrunk_to_the_room() {
        let p = fit(
            vec!["A".into(), "B".into(), "C".into(), "D".into()],
            &[0.5, 0.5, 0.5, 0.5],
        );
        let tall: f32 = p.lines.iter().map(|(_, s)| s * measure::LINE).sum();
        assert!((tall - measure::ROOM).abs() < 0.001, "{tall}");
        assert!(
            p.lines
                .iter()
                .all(|(_, s)| (*s - p.lines[0].1).abs() < 1e-6)
        );
    }

    #[test]
    fn layout_sizes_form_a_short_ladder() {
        assert_eq!(layout_size(4.0), 8.0);
        assert_eq!(layout_size(8.0), 8.0);
        assert_eq!(layout_size(16.0), 16.0);
        let mut seen = std::collections::BTreeSet::new();
        let mut size = 8.0_f32;
        while size < 180.0 {
            let at = layout_size(size);
            assert!(at >= size - 1e-3 && at / size < 1.13, "{size} → {at}");
            seen.insert((at * 100.0) as u32);
            size += 0.37;
        }
        assert!(seen.len() <= 28, "{}", seen.len());
    }

    fn ctx() -> Context {
        let ctx = Context::default();
        ctx.set_fonts(crate::theme::typography::font_definitions());
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        ctx
    }

    /// The display face measures as it does in the design's renders, so every poster is
    /// set as it is there. Reference sizes are percentages of the cover's width.
    /// A title, its lines with their sizes (% of the cover's width), and its raise (%).
    type PosterCase<'a> = (&'a str, &'a [(&'a str, f32)], f32);

    #[test]
    fn posters_match_the_design() {
        let ctx = ctx();
        let cases: [PosterCase<'_>; 7] = [
            ("Lantern Keep", &[("Lantern", 30.7), ("Keep", 46.19)], 3.54),
            (
                "Starfall Tactics",
                &[("Starfall", 31.91), ("Tactics", 33.63)],
                0.0,
            ),
            (
                "Low Tide Motel",
                &[("Low", 50.16), ("Tide", 46.63), ("Motel", 34.31)],
                0.0,
            ),
            ("Moss & Iron", &[("Moss", 44.68), ("& Iron", 38.43)], 0.0),
            (
                "The Long Winter Road: Director's Edition",
                &[
                    ("The Long", 21.5),
                    ("Winter", 28.63),
                    ("Road:", 33.9),
                    ("Director's", 20.04),
                    ("Edition", 27.04),
                ],
                0.0,
            ),
            (
                "Grand Theft Auto San Andreas",
                &[
                    ("Grand", 23.35),
                    ("Theft", 26.07),
                    ("Auto", 29.76),
                    ("San", 34.74),
                    ("Andreas", 17.19),
                ],
                0.0,
            ),
            ("Tidewater", &[("Tidewater", 23.08)], 0.0),
        ];
        for (title, want, raise) in cases {
            let p = poster(&ctx, title);
            assert_eq!(p.lines.len(), want.len(), "{title}");
            for ((text, size), (want_text, want_size)) in p.lines.iter().zip(want) {
                assert_eq!(text, want_text);
                assert!(
                    (size * 100.0 - want_size).abs() < 0.2,
                    "{title} / {text}: {} vs {want_size}",
                    size * 100.0
                );
            }
            assert!(
                (p.raise * 100.0 - raise).abs() < 0.05,
                "{title}: {}",
                p.raise
            );
        }
    }
}
