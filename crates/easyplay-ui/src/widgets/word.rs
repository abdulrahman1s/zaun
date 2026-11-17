//! Words: the quiet text buttons of the library's filter line and of the game page's meta
//! row (the scopes with their counts, "Filter", the sort, the protection level and tags
//! under a game's title). A word is text first: no fill until pointed at, then a faint one.
//!
//! - [`Word`] is the button;
//! - [`word_on`] is a filter that is on, with its own small remove button;
//! - [`word_text`], [`word_strong`] and [`word_rule`] are the inert parts of the line
//!   ("13 of 17", "3 selected", the hairline between scopes and filters).

use egui::{Align2, Color32, CornerRadius, Galley, Rect, Response, Sense, Ui, Widget, pos2, vec2};
use std::sync::Arc;

use super::{focus_ring, pointer_t, tooltip};
use crate::a11y;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{control, icon as icon_size, layout, radius, stroke};
use crate::theme::{self, Icon, Type, icons};

/// What leads a word.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lead {
    None,
    Icon(Icon),
    Dot(Color32),
}

/// A word of the filter line or of a game's meta row: 28 high, 8 padding, 13 / 500 in the
/// quiet text colour; under the pointer the text goes to primary over a faint fill.
#[must_use = "add it with `ui.add(word)`"]
pub struct Word<'a> {
    label: &'a str,
    count: Option<usize>,
    lead: Lead,
    caret: bool,
    pressed: Option<bool>,
    strong: bool,
    open: bool,
    icon_only: bool,
    tip: Option<(&'a str, Option<&'a str>)>,
}

impl<'a> Word<'a> {
    /// A word reading `label`.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            count: None,
            lead: Lead::None,
            caret: false,
            pressed: None,
            strong: false,
            open: false,
            icon_only: false,
            tip: None,
        }
    }

    /// A count after the label: 11 pt, quiet, tabular ("All 17").
    pub fn count(mut self, n: usize) -> Self {
        self.count = Some(n);
        self
    }

    /// A leading 13 pt glyph (the shield, the plus).
    pub fn icon(mut self, icon: Icon) -> Self {
        self.lead = Lead::Icon(icon);
        self
    }

    /// A leading tag dot.
    pub fn dot(mut self, color: Color32) -> Self {
        self.lead = Lead::Dot(color);
        self
    }

    /// A trailing caret: the word opens a menu.
    pub fn caret(mut self) -> Self {
        self.caret = true;
        self
    }

    /// Whether this word is the current choice (the scope shown): its text is primary.
    pub fn pressed(mut self, on: bool) -> Self {
        self.pressed = Some(on);
        self
    }

    /// Primary text at rest (the protection level and the tags under a game's title).
    pub fn strong(mut self) -> Self {
        self.strong = true;
        self
    }

    /// The word's menu is open: it keeps its pointed-at look.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Show only the icon, at 16 pt; the label stays the accessible name and becomes the
    /// tooltip. Needs [`Word::icon`].
    pub fn icon_only(mut self) -> Self {
        self.icon_only = matches!(self.lead, Lead::Icon(_));
        self
    }

    /// A tooltip, with the shortcut as a keycap when there is one.
    pub fn tip(mut self, text: &'a str, shortcut: Option<&'a str>) -> Self {
        self.tip = Some((text, shortcut));
        self
    }
}

/// The parts of a word, measured.
struct Parts {
    label: Option<Arc<Galley>>,
    count: Option<Arc<Galley>>,
    lead_w: f32,
    width: f32,
}

impl Word<'_> {
    /// The width the word will take (a line sizes the space around its words).
    pub fn width(&self, ui: &Ui) -> f32 {
        self.measure(ui, Color32::PLACEHOLDER, Color32::PLACEHOLDER)
            .width
    }

    fn measure(&self, ui: &Ui, text: Color32, quiet: Color32) -> Parts {
        let gap = layout::WORD_GAP;
        let label = (!self.icon_only).then(|| Type::BodyStrong.line(ui, self.label, text));
        let count = self
            .count
            .filter(|_| !self.icon_only)
            .map(|n| Type::Micro.numeric_galley(ui, &n.to_string(), quiet));
        let lead_w = match self.lead {
            Lead::None => 0.0,
            Lead::Icon(_) if self.icon_only => icon_size::INLINE,
            Lead::Icon(_) => icon_size::SMALL,
            Lead::Dot(_) => control::DOT,
        };
        let mut parts: Vec<f32> = Vec::with_capacity(4);
        if lead_w > 0.0 {
            parts.push(lead_w);
        }
        if let Some(g) = &label {
            parts.push(g.size().x);
        }
        if let Some(g) = &count {
            parts.push(g.size().x);
        }
        if self.caret && !self.icon_only {
            parts.push(icon_size::SMALL);
        }
        let inner: f32 = parts.iter().sum::<f32>() + gap * parts.len().saturating_sub(1) as f32;
        Parts {
            label,
            count,
            lead_w,
            width: inner + layout::WORD_PAD * 2.0,
        }
    }
}

impl Widget for Word<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = theme::theme(ui.ctx());
        let p = &theme.palette;
        // Laid out without a colour and tinted at paint time, so it can follow the pointer.
        let parts = self.measure(ui, Color32::PLACEHOLDER, Color32::PLACEHOLDER);
        let h = control::SM;
        let (rect, resp) = ui.allocate_exact_size(vec2(parts.width, h), Sense::click());
        // A count is part of the name ("All, 17"), as the button's text is in the design.
        let name = match self.count {
            Some(n) => format!("{}, {n}", self.label),
            None => self.label.to_owned(),
        };
        match self.pressed {
            Some(on) => a11y::toggle(&resp, &name, on),
            None => a11y::button(&resp, &name),
        }
        if ui.is_rect_visible(rect) {
            let enabled = ui.is_enabled();
            let hover = if enabled {
                pointer_t(&resp, &theme)
            } else {
                0.0
            };
            let held = hover.max(f32::from(u8::from(self.open)));
            let rest = if self.strong || self.pressed == Some(true) {
                p.t1
            } else {
                p.t3
            };
            let text = if !enabled {
                p.t4
            } else {
                mix(rest, p.t1, held)
            };
            let fill = fade(p.o1, held);
            let corner = CornerRadius::same(radius::WORD);
            let painter = ui.painter();
            painter.rect_filled(rect, corner, fill);
            let gap = layout::WORD_GAP;
            let cy = rect.center().y;
            let mut x = rect.left() + layout::WORD_PAD;
            match self.lead {
                Lead::None => {}
                Lead::Icon(icon) => {
                    // Under a game's title the shield stays quiet beside the primary level.
                    let glyph = if self.strong { p.t3 } else { text };
                    let size = parts.lead_w;
                    painter.text(
                        pos2(x + size / 2.0, cy),
                        Align2::CENTER_CENTER,
                        icon.text(false),
                        icons::font(size),
                        glyph,
                    );
                    x += size + gap;
                }
                Lead::Dot(c) => {
                    painter.circle_filled(pos2(x + control::DOT / 2.0, cy), control::DOT / 2.0, c);
                    x += control::DOT + gap;
                }
            }
            if let Some(g) = parts.label {
                let w = g.size().x;
                painter.galley(pos2(x, cy - g.size().y / 2.0), g, text);
                x += w + gap;
            }
            if let Some(g) = parts.count {
                let w = g.size().x;
                painter.galley(pos2(x, cy - g.size().y / 2.0), g, p.t3);
                x += w + gap;
            }
            if self.caret && !self.icon_only {
                painter.text(
                    pos2(x + icon_size::SMALL / 2.0, cy),
                    Align2::CENTER_CENTER,
                    icons::CARET_DOWN.text(false),
                    icons::font(icon_size::SMALL),
                    text,
                );
            }
            if resp.has_focus() {
                focus_ring(painter, rect, corner, &theme);
            }
        }
        match self.tip {
            Some((t, shortcut)) => tooltip::tooltip(resp, t, shortcut),
            None if self.icon_only => tooltip::tooltip(resp, self.label, None),
            None => resp,
        }
    }
}

/// The width [`word_on`] takes for `label` (with a tag's dot when `dot`).
pub fn word_on_width(ui: &Ui, label: &str, dot: bool) -> f32 {
    let gap = layout::WORD_GAP;
    let galley = Type::BodyStrong.line(ui, label, Color32::PLACEHOLDER);
    let dot_w = if dot { control::DOT + gap } else { 0.0 };
    layout::WORD_PAD + dot_w + galley.size().x + gap + control::WORD_X + control::WORD_X_END
}

/// The width [`word_rule`] takes.
pub fn word_rule_width() -> f32 {
    stroke::BORDER + layout::FILTER_RULE[1] * 2.0
}

/// A filter that is on: its label in primary text (after a tag's dot), then a small round
/// remove button named `remove_name`. The response is the remove button's.
pub fn word_on(ui: &mut Ui, label: &str, dot: Option<Color32>, remove_name: &str) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let gap = layout::WORD_GAP;
    let galley = Type::BodyStrong.line(ui, label, p.t1);
    let x_size = control::WORD_X;
    let w = word_on_width(ui, label, dot.is_some());
    let h = control::SM;
    let (rect, plain) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let x_rect = Rect::from_center_size(
        pos2(
            rect.right() - control::WORD_X_END - x_size / 2.0,
            rect.center().y,
        ),
        vec2(x_size, x_size),
    );
    // The hit area is the least pointer target; the round plate stays 18.
    let hit = Rect::from_center_size(x_rect.center(), egui::Vec2::splat(control::MIN_TARGET));
    let resp = ui.interact(hit, plain.id.with("remove"), Sense::click());
    a11y::button(&resp, remove_name);
    a11y::item(&plain, egui::accesskit::Role::Label, label);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let cy = rect.center().y;
        let mut x = rect.left() + layout::WORD_PAD;
        if let Some(c) = dot {
            painter.circle_filled(pos2(x + control::DOT / 2.0, cy), control::DOT / 2.0, c);
            x += control::DOT + gap;
        }
        painter.galley(pos2(x, cy - galley.size().y / 2.0), galley, p.t1);
        let hover = pointer_t(&resp, &theme);
        painter.circle_filled(x_rect.center(), x_size / 2.0, fade(p.o2, hover));
        painter.text(
            x_rect.center(),
            Align2::CENTER_CENTER,
            icons::CLOSE.text(false),
            icons::font(icon_size::SMALL),
            mix(p.t3, p.t1, hover),
        );
        if resp.has_focus() {
            focus_ring(painter, x_rect, CornerRadius::same(radius::PILL), &theme);
        }
    }
    tooltip::tooltip(resp, remove_name, None)
}

/// Inert text the height of a word, in quiet tabular figures ("13 of 17").
pub fn word_text(ui: &mut Ui, text: &str) -> Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let g = Type::BodyStrong.numeric_galley(ui, text, p.t3);
    let (rect, resp) = ui.allocate_exact_size(
        vec2(g.size().x + layout::WORD_PAD * 2.0, control::SM),
        Sense::hover(),
    );
    ui.painter().galley(
        pos2(
            rect.left() + layout::WORD_PAD,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        p.t3,
    );
    a11y::item(&resp, egui::accesskit::Role::Label, text);
    resp
}

/// Inert 13 / 600 primary text that leads a line ("3 selected"): 8 before it, 12 after.
pub fn word_strong(ui: &mut Ui, text: &str) -> Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let g = Type::BodyBold.numeric_galley(ui, text, p.t1);
    let after = theme::tokens::space::S3;
    let (rect, resp) = ui.allocate_exact_size(
        vec2(layout::WORD_PAD + g.size().x + after, control::SM),
        Sense::hover(),
    );
    ui.painter().galley(
        pos2(
            rect.left() + layout::WORD_PAD,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        p.t1,
    );
    a11y::item(&resp, egui::accesskit::Role::Label, text);
    resp
}

/// The hairline between the scopes and the filters: 1 × 14, 8 clear on each side.
pub fn word_rule(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let [h, _] = layout::FILTER_RULE;
    let (rect, _) = ui.allocate_exact_size(vec2(word_rule_width(), control::SM), Sense::hover());
    let r = Rect::from_center_size(rect.center(), vec2(stroke::BORDER, h));
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.line);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl FnOnce(&mut Ui) -> Rect) -> Rect {
        crate::widgets::test_frame(&theme::Appearance::default(), f)
    }

    #[test]
    fn a_word_is_28_high_with_8_padding() {
        let r = run(|ui| ui.add(Word::new("All").count(17)).rect);
        assert_eq!(r.height(), control::SM);
        let bare = run(|ui| ui.add(Word::new("All")).rect);
        assert!(r.width() > bare.width() + layout::WORD_GAP);
    }

    #[test]
    fn an_icon_only_word_is_the_glyph_and_its_padding() {
        let r = run(|ui| {
            ui.add(Word::new("Clear selection").icon(icons::CLOSE).icon_only())
                .rect
        });
        assert_eq!(r.width(), icon_size::INLINE + layout::WORD_PAD * 2.0);
    }

    #[test]
    fn the_rule_takes_its_margins() {
        let r = run(|ui| {
            word_rule(ui);
            ui.min_rect()
        });
        assert_eq!(r.width(), stroke::BORDER + layout::FILTER_RULE[1] * 2.0);
    }
}
