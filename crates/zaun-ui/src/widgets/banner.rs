//! The note (CSS `.note`): one tinted block above a page's tabs or in a dialog. Radius 10,
//! padded 10 10 10 12, a 16 pt glyph 10 before the text, the text 13 / 19:
//!
//! - a problem: `coral_tint` with a coral warning; a bold title, the error code in mono
//!   11 pt quiet text after it, the text under it, and up to four ghost text buttons under
//!   that ([`Note::problem`]);
//! - a missing program: `amber_tint` with an amber warning ([`Note::missing`]);
//! - still running in the background: `o1` with a `t2` clock ([`Note::lingering`]).
//!
//! The title of the last two runs into the text ("**Still running in the background.** The
//! window closed, …"). [`banner`] is an application banner (the shell's): a note of its
//! severity with its action and "Dismiss" as text buttons.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, CornerRadius, Rect, Sense, Ui, pos2, vec2};

use super::button::{Button, Variant};
use crate::a11y;
use crate::strings;
use crate::theme::tokens::{control, icon as icon_size, overlay, radius};
use crate::theme::{self, Icon, Type, icons};

/// Severity, which picks the note's tint and glyph (colour is never the only signal: the
/// words say it too).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Information: `o1`.
    Info,
    /// Warning: amber.
    Warning,
    /// Error / blocking: coral.
    Danger,
}

/// What the user did with a [`banner`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BannerResponse {
    /// The action button.
    pub action: bool,
    /// The dismiss button.
    pub dismissed: bool,
}

/// What the note looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    /// Coral: a problem.
    Problem,
    /// Amber: something missing.
    Missing,
    /// `o1` with a clock: still running in the background.
    Lingering,
    /// `o1` with a quiet warning: anything else to know.
    Info,
}

/// A note: build it with [`Note::problem`], [`Note::missing`], [`Note::lingering`] or
/// [`Note::new`], then [`Note::show`]. Returns which action was clicked.
#[must_use = "call `show`"]
#[derive(Clone, Copy, Debug)]
pub struct Note<'a> {
    tone: Tone,
    title: Option<&'a str>,
    code: Option<&'a str>,
    text: &'a str,
    actions: &'a [&'a str],
}

/// What the user did with a [`Note`].
#[derive(Clone, Debug)]
pub struct NoteResponse {
    /// The index of the action clicked, if any.
    pub clicked: Option<usize>,
    /// The note's own response: its whole area, carrying its accessible name and role.
    pub response: egui::Response,
}

impl<'a> Note<'a> {
    /// A problem (coral): a bold `title` on its own line, then `text`. Add the error code
    /// with [`Note::code`] and the fixes with [`Note::actions`].
    pub fn problem(title: &'a str, text: &'a str) -> Self {
        Self {
            tone: Tone::Problem,
            title: Some(title),
            code: None,
            text,
            actions: &[],
        }
    }

    /// A missing program (amber): "**This game's program can't be found.** It may have been
    /// moved…". `title` runs into `text`.
    pub fn missing(title: &'a str, text: &'a str) -> Self {
        Self {
            tone: Tone::Missing,
            title: Some(title),
            code: None,
            text,
            actions: &[],
        }
    }

    /// Still running in the background (`o1`, a clock). `title` runs into `text`.
    pub fn lingering(title: &'a str, text: &'a str) -> Self {
        Self {
            tone: Tone::Lingering,
            title: Some(title),
            code: None,
            text,
            actions: &[],
        }
    }

    /// A note of `severity` saying `text`.
    pub fn new(severity: Severity, text: &'a str) -> Self {
        Self {
            tone: match severity {
                Severity::Info => Tone::Info,
                Severity::Warning => Tone::Missing,
                Severity::Danger => Tone::Problem,
            },
            title: None,
            code: None,
            text,
            actions: &[],
        }
    }

    /// A bold title before the text.
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// The error code after the title ("ZN-SBX-010"), in mono 11 pt quiet text.
    pub fn code(mut self, code: &'a str) -> Self {
        self.code = Some(code);
        self
    }

    /// Up to four ghost text buttons under the text ("Edit permissions", "Try again",
    /// "View log", "Dismiss").
    pub fn actions(mut self, actions: &'a [&'a str]) -> Self {
        self.actions = actions;
        self
    }

    fn colors(&self, p: &theme::Palette) -> (Color32, Icon, Color32) {
        match self.tone {
            Tone::Problem => (p.coral_tint, icons::WARNING, p.coral),
            Tone::Missing => (p.amber_tint, icons::WARNING, p.amber),
            Tone::Lingering => (p.o1, icons::CLOCK, p.t2),
            Tone::Info => (p.o1, icons::WARNING, p.t2),
        }
    }

    fn job(&self, p: &theme::Palette, width: f32, text: &str) -> LayoutJob {
        let line = overlay::NOTE_LINE;
        let fmt = |t: Type, color: Color32| TextFormat {
            line_height: Some(line),
            valign: Align::Center,
            ..t.format(color, false)
        };
        let mut job = LayoutJob::default();
        // A problem's title stands on a line of its own, with its code; the others run on.
        let own_line =
            self.tone == Tone::Problem || self.code.is_some() || crate::i18n::bidi::has_rtl(text);
        if let Some(title) = self.title {
            job.append(
                &theme::typography::shaped(title),
                0.0,
                fmt(Type::BodyBold, p.t1),
            );
            if let Some(code) = self.code {
                let mut f = fmt(Type::Mono, p.t3);
                f.font_id.size = Type::Micro.size();
                job.append(code, overlay::NOTE_CODE_GAP, f);
            }
            if !self.text.is_empty() {
                let sep = if own_line { "\n" } else { " " };
                job.append(sep, 0.0, fmt(Type::Body, p.t1));
            }
        }
        // Right-to-left text arrives broken into lines and ordered (`wrap_text`).
        job.append(text, 0.0, fmt(Type::Body, p.t1));
        job.wrap.max_width = width.max(1.0);
        job
    }

    /// Show the note across the width at hand.
    pub fn show(self, ui: &mut Ui) -> NoteResponse {
        let theme = theme::theme(ui.ctx());
        let p = theme.palette.clone();
        let (fill, glyph, glyph_color) = self.colors(&p);
        let [left, top, right, bottom] = overlay::NOTE_PAD;
        let [acts_top, acts_left, acts_below, acts_gap] = overlay::NOTE_ACTIONS;
        let w = ui.available_width();
        let text_x = left + icon_size::INLINE + overlay::NOTE_GAP;
        let text_w = (w - text_x - right).max(1.0);
        let origin = ui.cursor().min;
        let plate = ui.painter().add(egui::Shape::Noop);
        let text = Type::Body.wrap_text(ui, self.text, text_w);
        let galley = ui.fonts_mut(|f| f.layout_job(self.job(&p, text_w, &text)));
        let text_h = galley.size().y;
        ui.painter()
            .galley(origin + vec2(text_x, top), galley, p.t1);
        let mut clicked = None;
        let mut h = top + text_h + bottom;
        if !self.actions.is_empty() {
            // Laid out here rather than by a wrapping layout: the buttons wrap onto further
            // lines when the note is narrow.
            let left_x = origin.x + text_x - acts_left;
            let right_x = origin.x + text_x + text_w;
            let mut at = pos2(left_x, origin.y + top + text_h + acts_top);
            for (i, a) in self.actions.iter().enumerate() {
                let b = Button::new(a).variant(Variant::Ghost);
                let bw = b.width(ui);
                if at.x > left_x && at.x + bw > right_x {
                    at = pos2(left_x, at.y + control::BUTTON + acts_gap);
                }
                let r = Rect::from_min_size(at, vec2(bw, control::BUTTON));
                if ui.put(r, b).clicked() {
                    clicked = Some(i);
                }
                at.x += bw + acts_gap;
            }
            h = at.y + control::BUTTON - origin.y + bottom - acts_below;
        }
        let rect = Rect::from_min_size(origin, vec2(w, h));
        // Placed over the whole note: the buttons above may have moved the cursor.
        let resp = ui.allocate_rect(rect, Sense::hover());
        ui.painter().set(
            plate,
            egui::Shape::rect_filled(rect, CornerRadius::same(radius::POP), fill),
        );
        ui.painter().text(
            pos2(
                rect.left() + left + icon_size::INLINE / 2.0,
                rect.top() + top + overlay::NOTE_GLYPH_DROP + icon_size::INLINE / 2.0,
            ),
            Align2::CENTER_CENTER,
            glyph.text(false),
            icons::font(icon_size::INLINE),
            glyph_color,
        );
        let spoken = match (self.title, self.code) {
            (Some(t), Some(c)) => format!("{t} ({c}). {}", self.text),
            (Some(t), None) => format!("{t} {}", self.text),
            _ => self.text.to_owned(),
        };
        let role = if self.tone == Tone::Problem || self.tone == Tone::Missing {
            egui::Role::Alert
        } else {
            egui::Role::Status
        };
        a11y::labelled(ui.ctx(), resp.id, role, &spoken);
        NoteResponse {
            clicked,
            response: resp,
        }
    }
}

/// Show a note of `severity` saying `text`, with `action` and (when `dismissible`)
/// "Dismiss" as text buttons under it. `dismissible` is false when the condition blocks
/// correctness.
pub fn banner(
    ui: &mut Ui,
    severity: Severity,
    text: &str,
    action: Option<&str>,
    dismissible: bool,
) -> BannerResponse {
    let mut labels: Vec<&str> = Vec::new();
    if let Some(a) = action {
        labels.push(a);
    }
    if dismissible {
        labels.push(strings::feedback::DISMISS());
    }
    let r = Note::new(severity, text).actions(&labels).show(ui);
    let mut out = BannerResponse::default();
    match r.clicked {
        Some(0) if action.is_some() => out.action = true,
        Some(_) => out.dismissed = true,
        None => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heights(width: f32) -> f32 {
        crate::widgets::test_frame(&theme::Appearance::default(), |ui| {
            ui.set_max_width(width);
            let top = ui.cursor().min.y;
            let _ = Note::problem("Title", "Text")
                .actions(&["Edit permissions", "Try again", "View log", "Dismiss"])
                .show(ui);
            ui.cursor().min.y - top
        })
    }

    #[test]
    fn actions_wrap_onto_further_lines_when_the_note_is_narrow() {
        let wide = heights(900.0);
        let narrow = heights(200.0);
        assert!(narrow >= wide + control::BUTTON, "{wide} → {narrow}");
    }

    #[test]
    fn show_returns_the_note_response() {
        let r = crate::widgets::test_frame(&theme::Appearance::default(), |ui| {
            Note::lingering("Still running in the background.", "Text").show(ui)
        });
        assert!(r.response.rect.height() > 0.0);
        assert_eq!(r.clicked, None);
    }
}
