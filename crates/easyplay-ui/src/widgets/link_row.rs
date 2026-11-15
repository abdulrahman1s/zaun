//! Link rows (UI_DESIGN_SYSTEM §7.5): a short list in which every row leads somewhere. A
//! row is an icon, a title, a value that starts on one axis down the list, and a glyph at
//! its end that says where it leads: a chevron for another part of the page, an arrow for
//! something outside EasyPlay. A row may list points beneath its value (what a game's
//! protection lets it reach). The whole row is the target. Rows are parted by hairlines that
//! step aside for the plate of the row being pointed at.

use std::sync::Arc;

use egui::accesskit::Role;
use egui::{
    Align, Align2, Color32, CornerRadius, Galley, Id, Layout, Rect, Response, Sense, Stroke, Ui,
    UiBuilder, pos2, vec2,
};

use super::{focus_ring, hover_t, press_t, skeleton, tooltip};
use crate::a11y;
use crate::theme::palette::{fade, mix};
use crate::theme::tokens::{icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Dur, Icon, Palette, Type, icons};

/// How a piece of a row is weighted, lightest first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tone {
    /// As usual.
    #[default]
    Normal,
    /// Worth a second look.
    Warning,
    /// Needs attention.
    Danger,
}

impl Tone {
    /// `normal`, or the colour of the weight.
    pub(super) fn color(self, normal: Color32, p: &Palette) -> Color32 {
        match self {
            Tone::Normal => normal,
            Tone::Warning => p.warning,
            Tone::Danger => p.danger,
        }
    }
}

/// Where a row leads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trailing {
    /// Another part of this page.
    #[default]
    Chevron,
    /// Something outside EasyPlay (a folder in the file manager).
    External,
}

/// One point beneath a row's value: an icon and a sentence.
#[derive(Clone, Copy, Debug)]
pub struct Point<'a> {
    /// What it is about.
    pub icon: Icon,
    /// The sentence.
    pub text: &'a str,
    /// Its weight.
    pub tone: Tone,
}

/// The points of a row.
#[derive(Clone, Copy, Debug, Default)]
pub enum Points<'a> {
    /// The row has none.
    #[default]
    None,
    /// Not known yet: room is kept for `lines` lines, drawn as a skeleton once `skeleton`.
    Pending {
        /// Lines to keep room for.
        lines: usize,
        /// Whether the wait is long enough to show it.
        skeleton: bool,
    },
    /// The points, named as a list for assistive technology, and a closing remark
    /// ("and 3 more").
    Listed {
        /// The list's accessible name.
        name: &'a str,
        /// The points.
        items: &'a [Point<'a>],
        /// What follows the last point.
        more: Option<&'a str>,
    },
}

/// One row.
#[derive(Clone, Copy, Debug)]
pub struct LinkRow<'a> {
    /// Leading icon.
    pub icon: Icon,
    /// Weight of the icon.
    pub icon_tone: Tone,
    /// What the row is about ("Protection").
    pub title: &'a str,
    /// What is true of it now ("Offline").
    pub value: &'a str,
    /// More about the value, set quieter after it ("in Games").
    pub detail: Option<&'a str>,
    /// Weight of the detail.
    pub detail_tone: Tone,
    /// Points beneath.
    pub points: Points<'a>,
    /// Where the row leads.
    pub trailing: Trailing,
    /// Accessible name ("Protection: Offline").
    pub name: &'a str,
    /// Tooltip, with its shortcut.
    pub hint: Option<(&'a str, Option<&'a str>)>,
}

impl<'a> LinkRow<'a> {
    /// A row leading to another part of the page.
    pub fn new(icon: Icon, title: &'a str, value: &'a str, name: &'a str) -> Self {
        Self {
            icon,
            icon_tone: Tone::Normal,
            title,
            value,
            detail: None,
            detail_tone: Tone::Normal,
            points: Points::None,
            trailing: Trailing::Chevron,
            name,
            hint: None,
        }
    }

    /// Set the detail.
    #[must_use]
    pub fn detail(mut self, detail: Option<&'a str>, tone: Tone) -> Self {
        self.detail = detail.filter(|d| !d.is_empty());
        self.detail_tone = tone;
        self
    }

    /// Weight the icon.
    #[must_use]
    pub fn icon_tone(mut self, tone: Tone) -> Self {
        self.icon_tone = tone;
        self
    }

    /// Set the points.
    #[must_use]
    pub fn points(mut self, points: Points<'a>) -> Self {
        self.points = points;
        self
    }

    /// The row opens something outside EasyPlay; `hint` says what, in a tooltip.
    #[must_use]
    pub fn external(mut self, hint: &'a str, shortcut: Option<&'a str>) -> Self {
        self.trailing = Trailing::External;
        self.hint = Some((hint, shortcut));
        self
    }
}

/// Room kept for the glyph at a row's end.
fn trail_w() -> f32 {
    icon_size::INLINE + space::S3
}

/// A cell of the points grid: where it is, and what it shows.
struct Cell {
    rect: Rect,
    icon: Option<(Icon, Color32)>,
    text: Option<Arc<Galley>>,
    label: Option<String>,
}

struct Laid {
    rect: Rect,
    resp: Response,
    line: Rect,
    title: Arc<Galley>,
    value: Arc<Galley>,
    value_at: egui::Pos2,
    detail: Option<(egui::Pos2, Arc<Galley>)>,
    cells: Vec<Cell>,
    skeleton: bool,
    cut: bool,
}

/// Cells of the given heights set in `cols` columns (one or two) that are read downwards:
/// the list is cut where the columns come out most even, the first the taller when two cuts
/// are as good. For each cell its column and its top, and the height of the taller column.
fn columns(heights: &[f32], cols: usize) -> (Vec<(usize, f32)>, f32) {
    let total: f32 = heights.iter().sum();
    let mut cut = heights.len();
    if cols > 1 {
        let (mut best, mut left) = (f32::INFINITY, 0.0);
        for (k, h) in heights.iter().enumerate() {
            left += h;
            let tallest = left.max(total - left);
            if tallest <= best {
                best = tallest;
                cut = k + 1;
            }
        }
    }
    let mut at = Vec::with_capacity(heights.len());
    let (mut top, mut tallest) = (0.0_f32, 0.0_f32);
    for (i, h) in heights.iter().enumerate() {
        if i == cut {
            top = 0.0;
        }
        at.push((usize::from(i >= cut), top));
        top += h;
        tallest = tallest.max(top);
    }
    (at, tallest)
}

fn lay(ui: &mut Ui, id: Id, row: &LinkRow<'_>, p: &Palette) -> Laid {
    let w = ui.available_width();
    let stacked = w < layout::LINK_ROW_STACK_BELOW;
    let text_h = Type::Body.line_height();
    let pad = (layout::LINK_ROW_H - text_h) / 2.0;
    let val_x = if stacked {
        layout::LINK_ROW_LEAD
    } else {
        layout::LINK_ROW_LEAD + layout::LINK_ROW_TITLE_W
    };
    let val_w = (w - val_x - trail_w()).max(1.0);
    let title_w = if stacked {
        val_w
    } else {
        layout::LINK_ROW_TITLE_W - space::S2
    };
    let title = Type::BodyStrong.galley(ui, row.title, p.text_primary, title_w, 1);
    let value = Type::Body.galley(ui, row.value, p.text_primary, val_w, 1);
    let rest = val_w - value.size().x - space::S2;
    let detail = row.detail.filter(|_| rest > space::S12).map(|d| {
        let c = row.detail_tone.color(p.text_secondary, p);
        Type::Body.galley(ui, d, c, rest, 1)
    });
    // Text that did not fit is given in full by the tooltip.
    let cut = value.elided
        || detail.as_ref().is_some_and(|g| g.elided)
        || (row.detail.is_some() && detail.is_none());
    let value_dy = if stacked { text_h + space::S0 } else { 0.0 };
    let mut h = pad + value_dy + text_h;

    // The points, laid out relative to the row's top-left corner.
    let grid_x = layout::LINK_ROW_LEAD;
    let grid_w = (w - grid_x - trail_w()).max(1.0);
    let cols = if grid_w >= 2.0 * layout::POINT_MIN_W + space::S6 {
        2
    } else {
        1
    };
    let col_w = (grid_w - space::S6 * (cols as f32 - 1.0)) / cols as f32;
    let text_x = icon_size::INLINE + space::S2;
    let mut cells: Vec<Cell> = Vec::new();
    let mut skeleton_lines = false;
    match row.points {
        Points::None => {}
        Points::Pending { lines, skeleton } => {
            skeleton_lines = skeleton;
            for _ in 0..lines * cols {
                cells.push(Cell {
                    rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(col_w, layout::POINT_H)),
                    icon: None,
                    text: None,
                    label: None,
                });
            }
        }
        Points::Listed { items, more, .. } => {
            for pt in items {
                let c = pt.tone.color(p.text_primary, p);
                let g = Type::Body.galley(ui, pt.text, c, col_w - text_x, 2);
                let ch = (g.size().y + space::S1 + space::S0).max(layout::POINT_H);
                cells.push(Cell {
                    rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(col_w, ch)),
                    icon: Some((pt.icon, pt.tone.color(p.text_secondary, p))),
                    text: Some(g),
                    label: Some(pt.text.to_owned()),
                });
            }
            if let Some(m) = more {
                let g = Type::Body.galley(ui, m, p.text_muted, col_w - text_x, 1);
                cells.push(Cell {
                    rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(col_w, layout::POINT_H)),
                    icon: None,
                    text: Some(g),
                    label: Some(m.to_owned()),
                });
            }
        }
    }
    let heights: Vec<f32> = cells.iter().map(|c| c.rect.height()).collect();
    let (at, grid_h) = if cells.is_empty() {
        (Vec::new(), 0.0)
    } else {
        columns(&heights, cols)
    };
    let grid_top = h + space::S1;
    if !cells.is_empty() {
        h = grid_top + grid_h + space::S3;
    } else {
        h += pad;
    }

    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    for (cell, (col, top)) in cells.iter_mut().zip(at) {
        let x = rect.left() + grid_x + col as f32 * (col_w + space::S6);
        cell.rect = cell.rect.translate(vec2(x, rect.top() + grid_top + top));
    }
    // The points first, so that the row lies over them and takes the pointer.
    if let Points::Listed { name, .. } = row.points {
        a11y::labelled(ui.ctx(), id.with("points"), Role::List, name);
        for (i, cell) in cells.iter().enumerate() {
            if let Some(label) = &cell.label {
                let r = ui.interact(cell.rect, id.with(("point", i)), Sense::hover());
                a11y::item(&r, Role::ListItem, label);
            }
        }
    }
    let target = rect.expand2(vec2(layout::LINK_ROW_OVERHANG, 0.0));
    let resp = ui.interact(target, id, Sense::click());
    a11y::button(&resp, row.name);
    let line = Rect::from_min_size(pos2(rect.left(), rect.top() + pad), vec2(w, text_h));
    let value_at = pos2(rect.left() + val_x, line.top() + value_dy);
    let detail = detail.map(|g| (pos2(value_at.x + value.size().x + space::S2, value_at.y), g));
    Laid {
        rect,
        resp,
        line,
        title,
        value,
        value_at,
        detail,
        cells,
        skeleton: skeleton_lines,
        cut,
    }
}

fn separator(ui: &Ui, rect: Rect, strength: f32, p: &Palette) {
    if strength > 0.0 {
        ui.painter().hline(
            rect.left() + layout::LINK_ROW_LEAD..=rect.right(),
            rect.top(),
            Stroke::new(stroke::BORDER, fade(p.border_subtle, strength)),
        );
    }
}

fn lead(ui: &Ui, rect: Rect, line: Rect, icon: Icon, color: Color32, title: &Arc<Galley>) {
    let painter = ui.painter();
    painter.text(
        pos2(rect.left() + icon_size::BUTTON / 2.0, line.center().y),
        Align2::CENTER_CENTER,
        icon.text(false),
        icons::font(icon_size::BUTTON),
        color,
    );
    painter.galley(
        pos2(rect.left() + layout::LINK_ROW_LEAD, line.top()),
        title.clone(),
        Color32::PLACEHOLDER,
    );
}

/// Show `rows` as one list. Returns the row that was activated.
pub fn link_rows(ui: &mut Ui, id: Id, rows: &[LinkRow<'_>]) -> Option<usize> {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let spacing = std::mem::replace(&mut ui.spacing_mut().item_spacing.y, 0.0);
    let laid: Vec<Laid> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| lay(ui, id.with(i), row, p))
        .collect();
    ui.spacing_mut().item_spacing.y = spacing;
    let hot: Vec<f32> = laid.iter().map(|l| hover_t(&l.resp, &theme)).collect();
    let nudge = if theme.motion.movement(Dur::Fast) > 0.0 {
        space::S0
    } else {
        0.0
    };
    let corner = CornerRadius::same(radius::ROW);
    let mut clicked = None;
    for (i, (row, l)) in rows.iter().zip(&laid).enumerate() {
        if l.resp.clicked() {
            clicked = Some(i);
        }
        if !ui.is_rect_visible(l.resp.rect) {
            continue;
        }
        let h = hot[i];
        let plate = l.resp.rect;
        if h > 0.0 {
            ui.painter()
                .rect_filled(plate, corner, fade(p.fill_hover, h));
        }
        let press = press_t(&l.resp, &theme);
        if press > 0.0 {
            ui.painter()
                .rect_filled(plate, corner, fade(p.fill_hover, press));
        }
        if i > 0 {
            separator(ui, l.rect, 1.0 - h.max(hot[i - 1]), p);
        }
        let icon_c = row
            .icon_tone
            .color(mix(p.text_secondary, p.text_primary, h), p);
        lead(ui, l.rect, l.line, row.icon, icon_c, &l.title);
        let painter = ui.painter();
        painter.galley(l.value_at, l.value.clone(), Color32::PLACEHOLDER);
        if let Some((at, g)) = &l.detail {
            painter.galley(*at, g.clone(), Color32::PLACEHOLDER);
        }
        let glyph = match row.trailing {
            Trailing::Chevron => icons::CARET_RIGHT,
            Trailing::External => icons::OPEN,
        };
        let slide = if row.trailing == Trailing::Chevron {
            nudge * h
        } else {
            0.0
        };
        painter.text(
            pos2(
                l.rect.right() - icon_size::INLINE / 2.0 + slide,
                l.line.center().y,
            ),
            Align2::CENTER_CENTER,
            glyph.text(false),
            icons::font(icon_size::INLINE),
            mix(p.text_muted, p.text_primary, h),
        );
        for (k, cell) in l.cells.iter().enumerate() {
            let text_x = cell.rect.left() + icon_size::INLINE + space::S2;
            let first_line = cell.rect.top() + (layout::POINT_H - Type::Body.line_height()) / 2.0;
            if let Some((icon, c)) = cell.icon {
                painter.text(
                    pos2(
                        cell.rect.left() + icon_size::INLINE / 2.0,
                        first_line + Type::Body.line_height() / 2.0,
                    ),
                    Align2::CENTER_CENTER,
                    icon.text(false),
                    icons::font(icon_size::INLINE),
                    c,
                );
            }
            match &cell.text {
                Some(g) => {
                    painter.galley(pos2(text_x, first_line), g.clone(), Color32::PLACEHOLDER);
                }
                None if l.skeleton => {
                    // Lines of uneven length, like sentences.
                    let part = [0.62, 0.48, 0.7, 0.4][k % 4];
                    skeleton::line(
                        ui,
                        pos2(cell.rect.left(), cell.rect.center().y - space::S1 - 1.0),
                        (cell.rect.width() * part).round(),
                        space::S2 + space::S0,
                    );
                }
                None => {}
            }
        }
        if l.resp.has_focus() {
            focus_ring(ui.painter(), plate.shrink(1.0), corner, &theme);
        }
    }
    for (row, l) in rows.iter().zip(laid) {
        let full = l.cut.then(|| match row.detail {
            Some(d) => format!("{} {d}", row.value),
            None => row.value.to_owned(),
        });
        match (full, row.hint) {
            (Some(full), Some((hint, shortcut))) => {
                tooltip::tooltip_at_pointer(l.resp, &format!("{full}\n{hint}"), shortcut);
            }
            (Some(full), None) => {
                tooltip::tooltip_at_pointer(l.resp, &full, None);
            }
            (None, Some((hint, shortcut))) => {
                tooltip::tooltip_at_pointer(l.resp, hint, shortcut);
            }
            (None, None) => {}
        }
    }
    clicked
}

/// A row of the same measure whose value is controls instead of a link (programs to run).
/// `first` is false when it follows another row.
pub fn control_row(ui: &mut Ui, icon: Icon, title: &str, first: bool, add: impl FnOnce(&mut Ui)) {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let w = ui.available_width();
    let stacked = w < layout::LINK_ROW_STACK_BELOW;
    let text_h = Type::Body.line_height();
    let pad = (layout::LINK_ROW_H - text_h) / 2.0;
    let h = if stacked {
        layout::LINK_ROW_H + layout::LINK_ROW_H - pad
    } else {
        layout::LINK_ROW_H
    };
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if !first {
        separator(ui, rect, 1.0, p);
    }
    let line = Rect::from_min_size(pos2(rect.left(), rect.top() + pad), vec2(w, text_h));
    let title_w = if stacked {
        w - layout::LINK_ROW_LEAD
    } else {
        layout::LINK_ROW_TITLE_W - space::S2
    };
    let g = Type::BodyStrong.galley(ui, title, p.text_primary, title_w, 1);
    lead(ui, rect, line, icon, p.text_secondary, &g);
    let cell = if stacked {
        Rect::from_min_max(
            pos2(rect.left() + layout::LINK_ROW_LEAD, line.bottom()),
            rect.max,
        )
    } else {
        Rect::from_min_max(
            pos2(
                rect.left() + layout::LINK_ROW_LEAD + layout::LINK_ROW_TITLE_W,
                rect.top(),
            ),
            rect.max,
        )
    };
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(cell)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            add(ui);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_fill_columns_downwards() {
        // Five points in two columns: three down the first, two down the second.
        let (at, h) = columns(&[26.0; 5], 2);
        assert_eq!(
            at,
            vec![(0, 0.0), (0, 26.0), (0, 52.0), (1, 0.0), (1, 26.0)]
        );
        assert!((h - 78.0).abs() < f32::EPSILON);
        // Tall points count for their height: the cut moves to keep the columns even.
        let (at, h) = columns(&[26.0, 46.0, 46.0, 26.0, 26.0], 2);
        assert_eq!(at[1], (0, 26.0));
        assert_eq!(at[2], (1, 0.0));
        assert!((h - 98.0).abs() < f32::EPSILON);
        let (at, h) = columns(&[46.0, 46.0, 26.0, 26.0, 26.0], 2);
        assert_eq!(at[2], (1, 0.0));
        assert!((h - 92.0).abs() < f32::EPSILON);
        // One column, and nothing at all.
        let (at, h) = columns(&[26.0, 26.0], 1);
        assert_eq!(at, vec![(0, 0.0), (0, 26.0)]);
        assert!((h - 52.0).abs() < f32::EPSILON);
        assert_eq!(columns(&[], 2), (Vec::new(), 0.0));
    }
}
