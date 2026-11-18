//! The bar chart (Statistics, Activity): a line with the tallest value at the left and
//! "Show as table" at the right; a plot 132 high between a hairline on top and a stronger
//! one under it; one column per bar, the bar at most 44 wide in the middle of its column,
//! 10 apart (3 past twelve bars), its top corners rounded. Bars are quiet; the latest bar
//! with play and the bar under the pointer are primary. Labels sit 7 under the plot; a dense
//! chart labels every fifth bar counting back from the newest. Bars grow from nothing when
//! the chart appears (10 ms apart), at full height under reduced motion. "Show as table"
//! swaps the plot for rows. The plot is one focusable figure: `←`/`→`, `Home`, `End` move
//! through the bars and announce them.

use egui::accesskit::Role;
use egui::{Align2, CornerRadius, Key, Modifiers, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use super::button::{Button, Size};
use super::{focus_ring, tooltip};
use crate::a11y::{self, Announcer, Politeness};
use crate::strings::stats as s;
use crate::theme::tokens::{control, layout, radius, stroke};
use crate::theme::{self, Type, motion};

/// One bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// Short label under the bar ("16 Nov", "Nov").
    pub label: String,
    /// What the bar is, with its value: the tooltip, the table row and what is announced
    /// ("16 Nov: 29 min", "November 2026: 15 h"). The part before ": " names the bar.
    pub spoken: String,
    /// The value as text ("29 min").
    pub value_text: String,
    /// Value.
    pub value: f64,
}

impl Bar {
    /// What the bar is, without its value ("16 Nov", "November 2026").
    pub fn when(&self) -> &str {
        match self.spoken.rsplit_once(": ") {
            Some((when, _)) if !when.is_empty() => when,
            _ => &self.label,
        }
    }
}

/// Options of [`bar_chart`].
#[derive(Clone, Copy, Debug)]
pub struct ChartOpts {
    /// Height of the plot (the design's is [`layout::CHART_H`]).
    pub height: f32,
    /// Draw the labels under the plot.
    pub axes: bool,
}

/// Whether a chart of `n` bars is dense: the gap closes up and labels thin out.
fn dense(n: usize) -> bool {
    n > layout::CHART_DENSE_FROM
}

/// Whether bar `i` of `n` carries a label: all of them, or every fifth counting back from
/// the newest when the chart is dense.
pub fn labelled(i: usize, n: usize) -> bool {
    !dense(n) || (n - 1 - i).is_multiple_of(layout::CHART_LABEL_EVERY)
}

/// The columns of a plot `rect` holding `n` bars, and the bar's width in each.
fn columns(rect: Rect, n: usize) -> (Vec<Rect>, f32) {
    let gap = if dense(n) {
        layout::CHART_GAP[1]
    } else {
        layout::CHART_GAP[0]
    };
    let col_w = ((rect.width() - gap * n.saturating_sub(1) as f32) / n.max(1) as f32).max(1.0);
    let cols = (0..n)
        .map(|i| {
            Rect::from_min_size(
                pos2(rect.left() + i as f32 * (col_w + gap), rect.top()),
                vec2(col_w, rect.height()),
            )
        })
        .collect();
    (cols, col_w.min(layout::CHART_BAR_MAX))
}

/// The full chart: the line above (the tallest value, "Show as table" / "Show as chart"),
/// then the plot with its labels or the table. `CHART_TOP` above it is included. `name` is
/// the plot's accessible description ([`s::chart_name`]); `table` is the toggle's state;
/// `focused` the keyboard's bar.
pub fn chart(
    ui: &mut Ui,
    name: &str,
    bars: &[Bar],
    table: &mut bool,
    focused: &mut Option<usize>,
) -> Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(layout::CHART_TOP);
    let (line, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), layout::CHART_HEAD),
        Sense::hover(),
    );
    let top = bars
        .iter()
        .max_by(|a, b| a.value.total_cmp(&b.value))
        .map_or_else(
            || crate::strings::fmt::play_time(0),
            |b| b.value_text.clone(),
        );
    let g = Type::Micro.numeric_galley(ui, &top, p.t3);
    ui.painter().galley(
        pos2(line.left(), line.center().y - g.size().y / 2.0),
        g,
        p.t3,
    );
    let label = if *table { s::SHOW_CHART } else { s::SHOW_TABLE };
    if small_button(ui, line, label()).clicked() {
        *table = !*table;
    }
    ui.add_space(layout::CHART_HEAD_GAP);
    if *table {
        bar_table(ui, name, bars);
        ui.allocate_response(egui::Vec2::ZERO, Sense::hover())
    } else {
        bar_chart(
            ui,
            name,
            bars,
            ChartOpts {
                height: layout::CHART_H,
                axes: true,
            },
            focused,
        )
    }
}

/// The chart's toggle: a quiet text button 22 high in 11 pt type, its text ending at the
/// right edge of `line` (the button reaches its padding past it).
fn small_button(ui: &mut Ui, line: Rect, label: &str) -> Response {
    let b = Button::ghost(label).size(Size::Xs);
    let w = b.width(ui);
    let rect = Rect::from_min_size(
        pos2(
            line.right() + control::BUTTON_PAD - w,
            line.center().y - control::BUTTON_XS / 2.0,
        ),
        vec2(w, control::BUTTON_XS),
    );
    // A child of its own, so reaching past the column does not widen the column.
    let mut c = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    c.add(b)
}

/// The plot (and its labels when `opts.axes`); `focused` is the keyboard's bar, kept by the
/// caller. `name` is the figure's accessible name.
pub fn bar_chart(
    ui: &mut Ui,
    name: &str,
    bars: &[Bar],
    opts: ChartOpts,
    focused: &mut Option<usize>,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let label_h = if opts.axes {
        layout::CHART_LABEL_GAP + Type::Micro.line_height()
    } else {
        0.0
    };
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(
        vec2(w, opts.height + label_h),
        Sense::focusable_noninteractive(),
    );
    a11y::labelled(ui.ctx(), resp.id, Role::Figure, name);
    let plot = Rect::from_min_size(rect.min, vec2(w, opts.height));
    let n = bars.len();
    let painter = ui.painter().clone();
    // The hairline on top and the stronger one under the plot.
    painter.hline(
        plot.x_range(),
        plot.top() + stroke::BORDER / 2.0,
        Stroke::new(stroke::BORDER, p.line),
    );
    painter.hline(
        plot.x_range(),
        plot.bottom() - stroke::BORDER / 2.0,
        Stroke::new(stroke::BORDER, p.line2),
    );
    if n == 0 {
        return resp;
    }
    // Keyboard.
    if resp.has_focus() {
        super::own_arrow_keys(&resp);
        let (l, r, home, end) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                i.consume_key(Modifiers::NONE, Key::Home),
                i.consume_key(Modifiers::NONE, Key::End),
            )
        });
        let cur = focused.unwrap_or(n - 1).min(n - 1);
        let next = if l {
            Some(cur.saturating_sub(1))
        } else if r {
            Some((cur + 1).min(n - 1))
        } else if home {
            Some(0)
        } else if end {
            Some(n - 1)
        } else if focused.is_none() {
            Some(cur)
        } else {
            None
        };
        if let Some(i) = next
            && *focused != Some(i)
        {
            *focused = Some(i);
            Announcer::announce(ui.ctx(), bars[i].spoken.clone(), Politeness::Polite);
        }
    }
    let max = bars
        .iter()
        .map(|b| b.value)
        .fold(0.0_f64, f64::max)
        .max(f64::MIN_POSITIVE);
    let latest = bars.iter().rposition(|b| b.value > 0.0);
    let (cols, bar_w) = columns(plot, n);
    // The bars grow again whenever a different period is shown.
    let grow_id = ui.make_persistent_id((
        "chart.grow",
        name,
        n,
        bars.first().map(|b| b.spoken.as_str()),
        bars.last().map(|b| b.label.as_str()),
    ));
    let keyboard = focused.filter(|_| resp.has_focus());
    let mut chosen = None;
    for (i, (b, col)) in bars.iter().zip(&cols).enumerate() {
        let full = (b.value / max) as f32 * plot.height();
        let grown = full * motion::bar_growth(ui.ctx(), grow_id, theme.motion, i);
        let x = col.center().x - bar_w / 2.0;
        let target = Rect::from_min_max(
            pos2(x, plot.bottom() - full),
            pos2(x + bar_w, plot.bottom()),
        );
        if keyboard == Some(i) {
            chosen = Some(target);
        }
        let hovered = full > 0.0 && {
            let r = ui.interact(target, resp.id.with(("bar", i)), Sense::hover());
            let on = r.hovered();
            tooltip::tooltip(r, &b.spoken, None);
            on
        };
        if grown > 0.0 {
            let bar = Rect::from_min_max(
                pos2(x, plot.bottom() - grown),
                pos2(x + bar_w, plot.bottom()),
            );
            let primary = hovered || latest == Some(i) || keyboard == Some(i);
            painter.rect_filled(
                bar,
                CornerRadius {
                    nw: radius::CHART,
                    ne: radius::CHART,
                    sw: 0,
                    se: 0,
                },
                if primary { p.t1 } else { p.t3 },
            );
        }
        if opts.axes && labelled(i, n) && !b.label.is_empty() {
            painter.text(
                pos2(col.center().x, plot.bottom() + layout::CHART_LABEL_GAP),
                Align2::CENTER_TOP,
                &b.label,
                Type::Micro.font_id(),
                p.t3,
            );
        }
    }
    if let (Some(i), Some(at)) = (keyboard, chosen) {
        // An empty day is a stub on the baseline: the tooltip stands under the plot.
        let at = if at.height() > 0.0 {
            at
        } else {
            Rect::from_x_y_ranges(at.x_range(), plot.y_range())
        };
        tooltip::at_rect(ui, resp.id, at, &bars[i].spoken);
    }
    if resp.has_focus() {
        focus_ring(&painter, plot, CornerRadius::same(radius::SM), &theme);
    }
    resp
}

/// The table alternative: one row per bar, 30 high over a hairline: what the bar is at the
/// left, its value at the right in tabular figures. A dense chart (the last 30 days) lists
/// only the bars with a value.
pub fn bar_table(ui: &mut Ui, name: &str, bars: &[Bar]) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let skip_empty = dense(bars.len());
    let resp = ui
        .scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for b in bars.iter().filter(|b| !skip_empty || b.value > 0.0) {
                let (rect, row) = ui.allocate_exact_size(
                    vec2(ui.available_width(), layout::CHART_TABLE_ROW),
                    Sense::hover(),
                );
                a11y::labelled(ui.ctx(), row.id, Role::Row, &b.spoken);
                let k = Type::Body.line(ui, b.when(), p.t1);
                ui.painter().galley(
                    pos2(rect.left(), rect.center().y - k.size().y / 2.0),
                    k,
                    p.t1,
                );
                let v = Type::Body.numeric_galley(ui, &b.value_text, p.t2);
                ui.painter().galley(
                    pos2(
                        rect.right() - v.size().x,
                        rect.center().y - v.size().y / 2.0,
                    ),
                    v,
                    p.t2,
                );
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - stroke::BORDER / 2.0,
                    Stroke::new(stroke::BORDER, p.line),
                );
            }
        })
        .response;
    a11y::labelled(ui.ctx(), resp.id, Role::Table, name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_charts_label_every_fifth_bar_from_the_newest() {
        assert!((0..7).all(|i| labelled(i, 7)));
        assert!((0..12).all(|i| labelled(i, 12)));
        let shown: Vec<usize> = (0..30).filter(|i| labelled(*i, 30)).collect();
        assert_eq!(shown, vec![4, 9, 14, 19, 24, 29]);
    }

    #[test]
    fn bars_are_at_most_44_wide_and_close_up_past_twelve() {
        let plot = Rect::from_min_size(pos2(0.0, 0.0), vec2(860.0, 132.0));
        let (cols, w) = columns(plot, 7);
        assert_eq!(w, layout::CHART_BAR_MAX);
        assert!((cols[1].left() - cols[0].right() - layout::CHART_GAP[0]).abs() < 0.01);
        assert!((cols[6].right() - 860.0).abs() < 0.01);
        let (cols, w) = columns(plot, 30);
        assert!(w < layout::CHART_BAR_MAX);
        assert!((cols[1].left() - cols[0].right() - layout::CHART_GAP[1]).abs() < 0.01);
    }

    #[test]
    fn a_bar_names_itself_without_its_value() {
        let b = Bar {
            label: "Nov".into(),
            spoken: "November 2026: 15 h".into(),
            value_text: "15 h".into(),
            value: 1.0,
        };
        assert_eq!(b.when(), "November 2026");
    }
}
