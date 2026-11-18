//! The rows pages are made of (UI_DESIGN_SYSTEM §7.5). Nothing here is a box:
//!
//! - a [`heading`] opens a group: the only tier between the display face and 13 pt text;
//! - a [`SettingRow`] is a label (with optional help) on the left and one control on the
//!   right;
//! - a [`fact`] is a quiet label and a plain value; a [`door`] is the same columns as one
//!   button that leads somewhere, with an arrow at the row's right edge;
//! - [`reach`] says what a game can and can't use, as two short lists;
//! - a [`table_row`] is a line of cells over a hairline.

use egui::{
    Align, Align2, Color32, CornerRadius, Id, Layout, Rect, Response, Sense, Stroke, Ui, UiBuilder,
    pos2, vec2,
};

use super::{focus_ring, hover_t, press_t};
use crate::a11y;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};

/// A section heading: 15 / 20 semibold, 32 above (none when `first`), 8 below.
pub fn heading(ui: &mut Ui, text: &str, first: bool) -> Response {
    heading_note(ui, text, None, false, first)
}

/// A section heading with a quiet note after it ("1 set"), or at the row's right end when
/// `split` ("Saved").
pub fn heading_note(
    ui: &mut Ui,
    text: &str,
    note: Option<&str>,
    split: bool,
    first: bool,
) -> Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    if !first {
        ui.add_space(layout::HEADING_GAP[0]);
    }
    let h = Type::Heading.line_height();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let title = Type::Heading.line(ui, text, p.t1);
    let title_w = title.size().x;
    ui.painter().galley(rect.min, title, p.t1);
    if let Some(note) = note {
        let g = Type::Caption.line(ui, note, p.t3);
        let x = if split {
            rect.right() - g.size().x
        } else {
            rect.left() + title_w + space::S2
        };
        ui.painter()
            .galley(pos2(x, rect.center().y - g.size().y / 2.0), g, p.t3);
    }
    a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, text);
    ui.add_space(layout::HEADING_GAP[1]);
    resp
}

/// Help text (12 / 16, quiet) wrapped at `width` as the design's `text-wrap: pretty` wraps
/// it: when the last line would hold a single word, the measure narrows until a word from
/// the line above joins it (without adding a line, and never below
/// [`layout::PRETTY_MIN`] of `width`).
pub fn help_galley(
    ui: &Ui,
    text: &str,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let set = |w: f32| Type::Caption.galley(ui, text, color, w, usize::MAX);
    let lone = |g: &egui::Galley| {
        g.rows.len() > 1
            && g.rows
                .last()
                .is_some_and(|r| !r.text().trim().contains(' '))
    };
    let first = set(width);
    if !lone(&first) {
        return first;
    }
    let rows = first.rows.len();
    let mut w = width - layout::PRETTY_STEP;
    while w >= width * layout::PRETTY_MIN {
        let g = set(w);
        if g.rows.len() != rows {
            break;
        }
        if !lone(&g) {
            return g;
        }
        w -= layout::PRETTY_STEP;
    }
    first
}

/// A setting row: label left, one control right, at least 38 high, no separators. A row
/// whose condition is unmet is drawn at 38 % and ignores input.
#[must_use = "call `show`"]
pub struct SettingRow<'a> {
    label: &'a str,
    help: Option<&'a str>,
    enabled: bool,
    mono: bool,
    id_salt: Option<Id>,
}

impl<'a> SettingRow<'a> {
    /// A row labelled `label`.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            help: None,
            enabled: true,
            mono: false,
            id_salt: None,
        }
    }

    /// Help under the label: 12 / 16, quiet, at most about 56 characters to a line.
    pub fn help(mut self, help: &'a str) -> Self {
        if !help.is_empty() {
            self.help = Some(help);
        }
        self
    }

    /// Whether the row's condition is met.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the label in the mono face (a program's file name).
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// Tell two rows with the same label in one group apart.
    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id_salt = Some(Id::unique(salt));
        self
    }

    /// Show the row; `control` adds what stands at its right end, left to right.
    pub fn show<R>(self, ui: &mut Ui, control: impl FnOnce(&mut Ui) -> R) -> R {
        self.show_with(ui, |_| {}, control)
    }

    /// Show the row with something of its own under the label (a small picker that belongs
    /// to the switch: which controllers, while controllers are on).
    pub fn show_with<R>(
        self,
        ui: &mut Ui,
        under: impl FnOnce(&mut Ui),
        control: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let p = theme::theme(ui.ctx()).palette.clone();
        let id = ui.make_persistent_id(("setting_row", self.label, self.id_salt));
        // The sizes of the control and of the label block are known from the last pass;
        // the first pass measures them and asks for another.
        let known: Option<(egui::Vec2, f32)> = ui.ctx().data(|d| d.get_temp(id));
        let (ctl, label_h) = known.unwrap_or((egui::Vec2::ZERO, Type::Body.line_height()));
        let full_w = ui.available_width();
        let gap = if ctl.x > 0.0 {
            layout::SETTING_GAP
        } else {
            0.0
        };
        let label_w = (full_w - ctl.x - gap).max(space::S16);
        let top = ui.cursor().min;
        let pad = layout::FACT_PAD_Y;
        let row_h = control::SETTING_ROW
            .max(label_h + pad * 2.0)
            .max(ctl.y + pad * 2.0);
        let enabled = self.enabled && ui.is_enabled();
        let mut out = None;
        ui.scope_builder(UiBuilder::new().id_salt(id), |ui| {
            if !enabled {
                ui.disable();
            }
            // The control is laid out before the label column, so it comes first in the
            // `Tab` order: a picker under the label (which belongs to the switch) follows it.
            // The control, at the right end, centred in the row. A centred layout counts
            // the whole height it is given as used, so the first pass measures the control
            // from the top; after that it is laid out, centred, in a rectangle of exactly
            // its own size.
            let ctl_rect = if known.is_some() {
                Rect::from_min_size(
                    pos2(top.x + full_w - ctl.x, top.y + (row_h - ctl.y) / 2.0),
                    vec2(ctl.x.max(1.0), ctl.y.max(1.0)),
                )
            } else {
                Rect::from_min_size(top, vec2(full_w, control::SETTING_ROW))
            };
            let mut right = ui.new_child(
                UiBuilder::new()
                    .id_salt("control")
                    .max_rect(ctl_rect)
                    .layout(Layout::left_to_right(if known.is_some() {
                        Align::Center
                    } else {
                        Align::Min
                    })),
            );
            if known.is_none() {
                right.set_invisible();
            }
            right.spacing_mut().item_spacing.x = space::S2;
            out = Some(control(&mut right));
            // The label column, centred in the row.
            let label_rect = Rect::from_min_size(
                pos2(top.x, top.y + (row_h - label_h) / 2.0),
                vec2(label_w, f32::INFINITY),
            );
            let mut left = ui.new_child(
                UiBuilder::new()
                    .id_salt("label")
                    .max_rect(label_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            left.spacing_mut().item_spacing.y = 0.0;
            let label = if self.mono && !crate::i18n::bidi::has_rtl(self.label) {
                // A program's file name: the mono face on the row's 18 pt line (an inline
                // `.mono` span in the design's 13 / 18 label). A right-to-left name keeps
                // the reordering of `Type::galley` (on the mono face's own 16 pt line).
                let mut job = Type::Mono.job(self.label, p.t1, label_w, usize::MAX);
                for s in &mut job.sections {
                    s.format.line_height = Some(Type::Body.line_height());
                }
                left.fonts_mut(|f| f.layout_job(job))
            } else {
                let style = if self.mono { Type::Mono } else { Type::Body };
                style.galley(&left, self.label, p.t1, label_w, usize::MAX)
            };
            let (r, _) = left.allocate_exact_size(label.size(), Sense::hover());
            left.painter().galley(r.min, label, p.t1);
            if let Some(help) = self.help {
                let g = help_galley(&left, help, p.t3, label_w.min(layout::HELP_MAX_W));
                let (r, _) = left.allocate_exact_size(g.size(), Sense::hover());
                left.painter().galley(r.min, g, p.t3);
            }
            under(&mut left);
            let measured_label_h = left.min_rect().height();
            let measured = (right.min_rect().size(), measured_label_h);
            let changed = known.is_none_or(|(c, l)| {
                (c - measured.0).length() > 0.5 || (l - measured.1).abs() > 0.5
            });
            if changed {
                ui.ctx().data_mut(|d| d.insert_temp(id, measured));
                ui.ctx()
                    .request_discard("a setting row measured its control");
            }
            ui.allocate_rect(
                Rect::from_min_size(top, vec2(full_w, row_h)),
                Sense::hover(),
            );
        });
        #[allow(clippy::expect_used)]
        out.expect("the row ran its control")
    }
}

/// How a page's label column is set for its width.
fn fact_columns(ui: &Ui) -> (f32, f32) {
    if ui.ctx().content_rect().width() < layout::NARROW_BELOW {
        (layout::FACT_LABEL_W_NARROW, layout::FACT_GAP_NARROW)
    } else {
        (layout::FACT_LABEL_W, layout::FACT_GAP)
    }
}

/// A fact: a quiet label and a plain value, 5 above and below. `value` is laid out in the
/// value column, which starts on the same axis down a list.
pub fn fact<R>(ui: &mut Ui, label: &str, value: impl FnOnce(&mut Ui) -> R) -> R {
    row(ui, label, layout::FACT_PAD_Y, None, value).1
}

/// A fact whose value is one run of text.
pub fn fact_text(ui: &mut Ui, label: &str, value: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    fact(ui, label, |ui| {
        Type::Body.paragraph(ui, value, p.t1);
    });
}

/// Where a door leads, which is the glyph at its right edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leads {
    /// To a tab of this page.
    Tab,
    /// Outside Zaun (the file manager).
    Outside,
}

/// A door: a whole-row button with the columns of a [`fact`] and an arrow at the row's
/// right edge. It reaches 8 past the text on both sides, so label and value stay on the
/// column; `name` is what assistive technology reads.
pub fn door(
    ui: &mut Ui,
    label: &str,
    name: &str,
    leads: Leads,
    value: impl FnOnce(&mut Ui),
) -> Response {
    let (resp, ()) = row(ui, label, layout::DOOR_PAD, Some((leads, name)), value);
    resp
}

fn row<R>(
    ui: &mut Ui,
    label: &str,
    pad: f32,
    door: Option<(Leads, &str)>,
    value: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let (label_w, gap) = fact_columns(ui);
    let arrow_w = icon_size::SMALL;
    let full_w = ui.available_width();
    let top = ui.cursor().min;
    let id = ui.make_persistent_id(("row", label, door.is_some()));
    // The plate is painted under content laid out after it: reserve its place.
    let plate = ui.painter().add(egui::Shape::Noop);
    let value_x = top.x + label_w + gap;
    let value_w = (full_w - label_w - gap - gap - arrow_w).max(space::S16);
    let mut col = ui.new_child(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(Rect::from_min_size(
                pos2(value_x, top.y + pad),
                vec2(value_w, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    col.spacing_mut().item_spacing.y = 0.0;
    let out = value(&mut col);
    // The label wraps to at most two lines: the row is as tall as the taller column.
    let k = Type::Body.galley(ui, label, p.t3, label_w, 2);
    let h = col
        .min_rect()
        .height()
        .max(k.size().y)
        .max(Type::Body.line_height())
        + pad * 2.0;
    let rect = Rect::from_min_size(top, vec2(full_w, h));
    let sense = if door.is_some() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, id, sense);
    ui.allocate_rect(rect, Sense::hover());
    ui.painter().galley(pos2(top.x, top.y + pad), k, p.t3);
    if let Some((leads, name)) = door {
        a11y::button(&resp, name);
        let hover = hover_t(&resp, &theme);
        let pressed = press_t(&resp, &theme);
        let over = rect.expand2(vec2(layout::ROW_OVERHANG, 0.0));
        let corner = CornerRadius::same(radius::ROW);
        let fill = mix(mix(Color32::TRANSPARENT, p.o1, hover), p.o2, pressed);
        ui.painter()
            .set(plate, egui::Shape::rect_filled(over, corner, fill));
        let glyph = match leads {
            Leads::Tab => icons::CARET_RIGHT,
            Leads::Outside => icons::OPEN,
        };
        ui.painter().text(
            pos2(
                rect.right() - arrow_w / 2.0,
                top.y + pad + Type::Body.line_height() / 2.0,
            ),
            Align2::CENTER_CENTER,
            glyph.text(false),
            icons::font(icon_size::SMALL),
            mix(p.t3, p.t1, hover),
        );
        if resp.has_focus() {
            focus_ring(ui.painter(), over, corner, &theme);
        }
    }
    (resp, out)
}

/// What a game can and can't use, as two short lists: quiet labels, then the items as a
/// comma list. `cant` empty reads "Nothing is held back" (the caller passes that sentence).
pub fn reach(ui: &mut Ui, can_label: &str, can: &str, cant_label: &str, cant: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let a = Type::Body.line(ui, can_label, p.t3);
    let b = Type::Body.line(ui, cant_label, p.t3);
    let label_w = a.size().x.max(b.size().x);
    let text_x = label_w + space::S3;
    let text_w = (w - text_x).max(space::S16);
    let top = ui.cursor().min;
    let mut y = top.y;
    for (label, text) in [(a, can), (b, cant)] {
        let g = Type::Body.galley(ui, text, p.t2, text_w, usize::MAX);
        ui.painter().galley(pos2(top.x, y), label, p.t3);
        let h = g.size().y;
        ui.painter().galley(pos2(top.x + text_x, y), g, p.t2);
        y += h + space::S0;
    }
    let (_, resp) = ui.allocate_exact_size(vec2(w, y - space::S0 - top.y), Sense::hover());
    // A text node, as every other read-only sentence: its words are what it says.
    a11y::item(
        &resp,
        egui::Role::Label,
        &format!("{can_label}: {can}. {cant_label}: {cant}."),
    );
}

/// The width of a table column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Col {
    /// A share of what the fixed columns leave (`minmax(0, 1fr)`; 1.5 for a wider share).
    Flex(f32),
    /// A fixed width.
    Fixed(f32),
}

/// The rectangles of a table's cells in a row `height` high starting at `top_left`.
pub fn table_cells(top_left: egui::Pos2, width: f32, height: f32, cols: &[Col]) -> Vec<Rect> {
    let gaps = layout::LIST_COL_GAP * cols.len().saturating_sub(1) as f32;
    let fixed: f32 = cols
        .iter()
        .map(|c| match c {
            Col::Fixed(w) => *w,
            Col::Flex(_) => 0.0,
        })
        .sum();
    let shares: f32 = cols
        .iter()
        .map(|c| match c {
            Col::Flex(s) => *s,
            Col::Fixed(_) => 0.0,
        })
        .sum();
    let free = (width - gaps - fixed).max(0.0);
    let mut x = top_left.x;
    cols.iter()
        .map(|c| {
            let w = match c {
                Col::Fixed(w) => *w,
                Col::Flex(s) => free * s / shares.max(f32::EPSILON),
            };
            let r = Rect::from_min_size(pos2(x, top_left.y), vec2(w, height));
            x += w + layout::LIST_COL_GAP;
            r
        })
        .collect()
}

/// A table row: 34 high (a header 28, in 12 pt quiet text), a hairline under it. `cell` is
/// called for each column with a UI laid out left to right, centred on the row; text added
/// through [`table_text`] takes the row's colours.
pub fn table_row(
    ui: &mut Ui,
    cols: &[Col],
    header: bool,
    mut cell: impl FnMut(&mut Ui, usize),
) -> Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let h = if header {
        layout::TABLE_HEAD
    } else {
        layout::TABLE_ROW
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    for (i, r) in table_cells(rect.min, rect.width(), h, cols)
        .into_iter()
        .enumerate()
    {
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("cell", i))
                .max_rect(r)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(r.intersect(ui.clip_rect()));
        cell(&mut child, i);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - stroke::BORDER / 2.0,
        Stroke::new(stroke::BORDER, p.line),
    );
    resp
}

/// Text of a table cell: the first cell of a row is primary, the rest secondary; a header
/// is 12 pt medium and quiet throughout. `numeric` sets tabular figures.
pub fn table_text(ui: &mut Ui, text: &str, column: usize, header: bool, numeric: bool) -> Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (style, color) = if header {
        (Type::Label, p.t3)
    } else if column == 0 {
        (Type::Body, p.t1)
    } else {
        (Type::Body, p.t2)
    };
    let rich = if numeric {
        style.numeric(text)
    } else {
        style.rich(text)
    };
    ui.add(egui::Label::new(rich.color(color)).truncate())
}

/// A glyph at the 13 pt inline size, vertically centred on a line of body text: carets, the
/// shield beside the protection level.
pub fn inline_icon(ui: &mut Ui, icon: Icon, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(icon_size::SMALL, Type::Body.line_height()),
        Sense::hover(),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.text(false),
        icons::font(icon_size::SMALL),
        color,
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_text_does_not_leave_one_word_on_its_last_line() {
        let texts = [
            "Last used UMU-Proton 10.0-4. Storage is prepared with the same version.",
            "Deletes every recorded session on this computer. Your games are not touched.",
        ];
        let last = |g: &egui::Galley| g.rows.last().map(|r| r.text()).unwrap_or_default();
        for text in texts {
            let lone_widths = crate::widgets::test_frame(&Default::default(), |ui| {
                let mut out = Vec::new();
                for w in (300..=424).step_by(2) {
                    let w = w as f32;
                    let plain = Type::Caption.galley(ui, text, Color32::WHITE, w, usize::MAX);
                    if plain.rows.len() > 1 && !last(&plain).trim().contains(' ') {
                        out.push((
                            w,
                            plain.rows.len(),
                            help_galley(ui, text, Color32::WHITE, w),
                        ));
                    }
                }
                out
            });
            assert!(!lone_widths.is_empty(), "{text}: no width leaves one word");
            for (w, rows, pretty) in lone_widths {
                assert_eq!(pretty.rows.len(), rows, "{text} at {w}");
                assert!(
                    last(&pretty).trim().contains(' '),
                    "{w}: {:?}",
                    last(&pretty)
                );
            }
        }
    }

    #[test]
    fn table_columns_share_what_the_fixed_ones_leave() {
        let cols = [
            Col::Flex(1.0),
            Col::Fixed(70.0),
            Col::Fixed(100.0),
            Col::Fixed(130.0),
        ];
        let cells = table_cells(pos2(10.0, 0.0), 720.0, 34.0, &cols);
        assert_eq!(cells.len(), 4);
        assert!((cells[0].width() - (720.0 - 300.0 - 48.0)).abs() < 0.01);
        assert!((cells[3].right() - 730.0).abs() < 0.01);
        assert!((cells[1].left() - cells[0].right() - 16.0).abs() < 0.01);
        let shares = table_cells(
            pos2(0.0, 0.0),
            516.0,
            34.0,
            &[Col::Flex(1.5), Col::Fixed(100.0), Col::Flex(1.0)],
        );
        assert!((shares[0].width() / shares[2].width() - 1.5).abs() < 0.01);
    }
}
