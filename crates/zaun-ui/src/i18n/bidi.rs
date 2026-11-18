//! Right-to-left text for an engine that has no bidirectional layout (ADR-0032).
//!
//! egui shapes each run of one font face on its own and places the runs left to right in
//! string order. A run that starts with an Arabic letter and comes wholly from the Arabic
//! face is shaped right to left by itself. So before text reaches egui we put its runs in
//! visual order with the Unicode Bidirectional Algorithm and leave each Arabic-face piece
//! in logical order. This holds because every Arabic-script character comes from the
//! Arabic face and no other character does (tests in `theme::typography` pin both).

use std::borrow::Cow;

use unicode_bidi::{BidiInfo, Level};

use super::Dir;

/// Whether `text` has any right-to-left character.
pub fn has_rtl(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
    })
}

/// Whether `c` is drawn by the Arabic face (and only by it).
pub fn arabic_face(c: char) -> bool {
    matches!(c as u32,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x0870..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        other => other,
    }
}

/// One line of text (a `'\n'` splits it into lines handled one by one), put in the order
/// egui must be given so that it reads correctly. `base` is the paragraph's direction;
/// `None` takes it from the first strong character.
pub fn visual_line(text: &str, base: Option<Dir>) -> Cow<'_, str> {
    if !has_rtl(text) {
        return Cow::Borrowed(text);
    }
    if text.contains('\n') {
        let lines: Vec<String> = text
            .split('\n')
            .map(|l| visual_line(l, base).into_owned())
            .collect();
        return Cow::Owned(lines.join("\n"));
    }
    let level = match base {
        Some(Dir::Rtl) => Some(Level::rtl()),
        Some(Dir::Ltr) => Some(Level::ltr()),
        None => None,
    };
    let info = BidiInfo::new(text, level);
    let Some(para) = info.paragraphs.first() else {
        return Cow::Borrowed(text);
    };
    let (levels, runs) = info.visual_runs(para, para.range.clone());
    let mut out = String::with_capacity(text.len());
    for run in runs {
        let piece = &text[run.clone()];
        if levels[run.start].is_ltr() {
            out.push_str(piece);
        } else {
            push_reversed_run(&mut out, piece);
        }
    }
    Cow::Owned(out)
}

/// Appends a right-to-left run: pieces in reverse order, Arabic-face pieces as they are
/// (the engine reverses them), others with characters reversed and brackets mirrored.
fn push_reversed_run(out: &mut String, run: &str) {
    let mut pieces: Vec<(bool, &str)> = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (i, c) in run.char_indices() {
        let is_arabic = arabic_face(c);
        match current {
            Some(k) if k == is_arabic => {}
            Some(k) => {
                pieces.push((k, &run[start..i]));
                start = i;
                current = Some(is_arabic);
            }
            None => current = Some(is_arabic),
        }
    }
    if let Some(k) = current {
        pieces.push((k, &run[start..]));
    }
    for (is_arabic, piece) in pieces.into_iter().rev() {
        if is_arabic {
            out.push_str(piece);
        } else {
            out.extend(piece.chars().rev().map(mirror));
        }
    }
}

/// Several lines: `text` broken into lines no wider than `max_width` (at most `max_rows`,
/// the last ending in "…" when cut), each put in visual order, joined with `'\n'`. The
/// caller lays the result out with wrapping off. Breaks are at spaces, in logical order.
pub fn wrapped(
    measure: &mut dyn FnMut(&str) -> f32,
    text: &str,
    max_width: f32,
    max_rows: usize,
    base: Option<Dir>,
) -> String {
    wrapped_cut(measure, text, max_width, max_rows, base).0
}

/// [`wrapped`] that also tells whether the text was cut (the last row ends in "…" that is
/// not in the source), so a caller can show the full text in a tooltip.
pub fn wrapped_cut(
    measure: &mut dyn FnMut(&str) -> f32,
    text: &str,
    max_width: f32,
    max_rows: usize,
    base: Option<Dir>,
) -> (String, bool) {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ').filter(|w| !w.is_empty()) {
            if line.is_empty() {
                line.push_str(word);
                continue;
            }
            let candidate = format!("{line} {word}");
            if measure(&visual_line(&candidate, base)) <= max_width {
                line = candidate;
            } else {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            }
        }
        lines.push(line);
    }
    let rows = max_rows.max(1);
    let mut cut = lines.len() > rows;
    lines.truncate(rows);
    let full = lines.len() == rows;
    if let Some(last) = lines.last_mut() {
        // The last allowed row is cut when text was dropped or one word overflows it.
        let overflows = full && measure(&visual_line(last, base)) > max_width;
        if cut || overflows {
            while !last.is_empty() && measure(&visual_line(&format!("{last}…"), base)) > max_width
            {
                last.pop();
            }
            last.push('…');
            cut = true;
        }
    }
    let out = lines
        .iter()
        .map(|l| visual_line(l, base).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    (out, cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> String {
        visual_line(text, Some(Dir::Rtl)).into_owned()
    }

    #[test]
    fn text_without_rtl_is_borrowed() {
        assert!(matches!(
            visual_line("Play", Some(Dir::Rtl)),
            Cow::Borrowed("Play")
        ));
    }

    #[test]
    fn words_come_out_in_visual_order() {
        assert_eq!(v("إضافة لعبة"), "لعبة إضافة");
    }

    #[test]
    fn latin_runs_stay_left_to_right() {
        assert_eq!(v("حذف Lantern Keep؟"), "؟Lantern Keep حذف");
        assert_eq!(v("12 لعبة"), "لعبة 12");
    }

    #[test]
    fn brackets_are_mirrored() {
        assert_eq!(v("لعبة (جديدة)"), "(جديدة) لعبة");
    }

    #[test]
    fn auto_direction_follows_the_first_strong_letter() {
        assert_eq!(
            visual_line("Lantern Keep تمت إضافتها", None),
            "Lantern Keep إضافتها تمت"
        );
    }

    #[test]
    fn wrapped_keeps_reading_order_and_width() {
        let text = "واحد اثنان ثلاثة اربعة خمسة";
        let mut measure = |s: &str| s.chars().count() as f32;
        let out = wrapped(&mut measure, text, 12.0, 10, Some(Dir::Rtl));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.len() > 1, "{out}");
        for l in &lines {
            assert!(l.chars().count() <= 12, "{l}");
        }
        for w in text.split(' ') {
            assert_eq!(out.matches(w).count(), 1, "{w} in {out}");
        }
        // The first line holds the first words, in visual order.
        assert!(
            lines[0].starts_with("اثنان") && lines[0].ends_with("واحد"),
            "{out}"
        );
    }

    #[test]
    fn wrapped_cuts_the_last_row_with_an_ellipsis() {
        let mut measure = |s: &str| s.chars().count() as f32;
        let out = wrapped(&mut measure, "aa bb cc dd ee ff", 5.0, 2, None);
        assert_eq!(out.lines().count(), 2);
        assert!(out.ends_with('…'), "{out}");
    }

    /// The assumption the design rests on: the text engine draws a run of Arabic letters
    /// right to left by itself.
    mod engine {
        use super::super::*;
        use crate::theme::typography::{Type, font_definitions};
        use egui::Color32;

        fn ctx() -> egui::Context {
            crate::theme::typography::test_context(font_definitions())
        }

        /// The x of the first glyph of each char, in the galley's first row.
        fn xs(ctx: &egui::Context, text: &str) -> Vec<(char, f32)> {
            let galley = ctx.fonts_mut(|f| {
                f.layout_no_wrap(text.to_owned(), Type::Body.font_id(), Color32::WHITE)
            });
            let row = &galley.rows[0];
            let mut out = Vec::new();
            for g in &row.glyphs {
                out.push((g.chr, g.pos.x));
            }
            out
        }

        fn x_of(glyphs: &[(char, f32)], c: char) -> f32 {
            glyphs
                .iter()
                .find(|(ch, _)| *ch == c)
                .map(|(_, x)| *x)
                .unwrap_or(f32::NAN)
        }

        #[test]
        fn an_arabic_run_is_drawn_right_to_left_by_the_engine() {
            let ctx = ctx();
            let g = xs(&ctx, "اب");
            let (alef, beh) = (x_of(&g, 'ا'), x_of(&g, 'ب'));
            assert!(alef > beh, "alef at {alef}, beh at {beh}: {g:?}");
        }

        #[test]
        fn a_number_before_an_arabic_word_stands_to_its_right() {
            let ctx = ctx();
            let line = visual_line("تمت إضافة Lantern Keep (12 لعبة)", Some(Dir::Rtl)).into_owned();
            let g = xs(&ctx, &line);
            // Reading right to left: the number, then the word it counts.
            assert!(x_of(&g, '1') > x_of(&g, 'ل'), "{g:?}");
            // "Lantern" reads left to right inside the sentence.
            assert!(x_of(&g, 'L') < x_of(&g, 'K'), "{g:?}");
            // "Lantern Keep" comes after (to the left of) "تمت إضافة".
            assert!(x_of(&g, 'K') < x_of(&g, 'ت'), "{g:?}");
        }

        #[test]
        fn visual_line_puts_the_first_word_on_the_right() {
            let ctx = ctx();
            let line = visual_line("اب جد", Some(Dir::Rtl)).into_owned();
            let g = xs(&ctx, &line);
            let first = x_of(&g, 'ا').min(x_of(&g, 'ب'));
            let second = x_of(&g, 'ج').max(x_of(&g, 'د'));
            assert!(first > second, "{g:?}");
        }
    }
}
