//! Bar chart (UI_DESIGN_SYSTEM §7.6): bars in the accent at 80 %, the hovered or focused bar
//! at 100 % with its value; a baseline and at most three guides in `border.subtle`; x labels
//! at readable intervals. One focusable response with an internal index: `←`/`→`, `Home`,
//! `End` move the focused bar and announce "Tuesday 3 November: 2 h 10 min".

use egui::accesskit::Role;
use egui::{Align2, CornerRadius, Key, Modifiers, Response, Sense, Stroke, Ui, pos2, vec2};

use super::focus_ring;
use crate::a11y::{self, Announcer, Politeness};
use crate::theme::palette::fade;
use crate::theme::tokens::{radius, space, stroke};
use crate::theme::{self, Type};

/// One bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// Short axis label ("3 Nov", "Tue").
    pub label: String,
    /// Full spoken label with the value ("Tuesday 3 November: 2 h 10 min").
    pub spoken: String,
    /// Value label shown on focus ("2 h 10 min").
    pub value_text: String,
    /// Value.
    pub value: f64,
}

/// Options.
#[derive(Clone, Copy, Debug)]
pub struct ChartOpts {
    /// Height of the plot.
    pub height: f32,
    /// Draw guides and x labels.
    pub axes: bool,
}

/// Show the chart; `focused` is the keyboard/hover index kept by the caller. `name` is the
/// accessible name of the chart.
pub fn bar_chart(
    ui: &mut Ui,
    name: &str,
    bars: &[Bar],
    opts: ChartOpts,
    focused: &mut Option<usize>,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let label_h = if opts.axes { space::S6 } else { 0.0 };
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, opts.height + label_h), Sense::click());
    a11y::labelled(ui.ctx(), resp.id, Role::Figure, name);
    let plot = egui::Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - label_h));
    let n = bars.len();
    if n == 0 {
        return resp;
    }
    // Keyboard.
    if resp.has_focus() {
        crate::widgets::own_arrow_keys(&resp);
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
    let slot = plot.width() / n as f32;
    let gap = (slot * 0.25).clamp(1.0, space::S2);
    let hover = resp
        .hover_pos()
        .filter(|h| plot.contains(*h))
        .map(|h| (((h.x - plot.left()) / slot) as usize).min(n - 1));
    if let Some(h) = hover {
        *focused = Some(h);
    }
    let max = bars
        .iter()
        .map(|b| b.value)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let painter = ui.painter_at(rect.expand(space::S2));
    if opts.axes {
        for k in 1..=3 {
            let y = plot.bottom() - plot.height() * k as f32 / 3.0;
            painter.hline(
                plot.x_range(),
                y,
                Stroke::new(stroke::BORDER, fade(p.border_subtle, 0.6)),
            );
        }
    }
    painter.hline(
        plot.x_range(),
        plot.bottom(),
        Stroke::new(stroke::BORDER, p.border_subtle),
    );
    let active = focused.filter(|_| resp.has_focus() || hover.is_some());
    for (i, b) in bars.iter().enumerate() {
        let h = (b.value / max) as f32 * (plot.height() - space::S5);
        let x = plot.left() + i as f32 * slot + gap / 2.0;
        let bar = egui::Rect::from_min_max(
            pos2(
                x,
                plot.bottom() - h.max(if b.value > 0.0 { 2.0 } else { 0.0 }),
            ),
            pos2(x + slot - gap, plot.bottom()),
        );
        let on = active == Some(i);
        let color = if on { p.accent } else { fade(p.accent, 0.8) };
        painter.rect_filled(
            bar,
            CornerRadius {
                nw: radius::XS,
                ne: radius::XS,
                sw: 0,
                se: 0,
            },
            color,
        );
        if on {
            painter.text(
                pos2(
                    bar.center()
                        .x
                        .clamp(plot.left() + 30.0, plot.right() - 30.0),
                    bar.top() - space::S1,
                ),
                Align2::CENTER_BOTTOM,
                &b.value_text,
                Type::Caption.font_id(),
                p.text_primary,
            );
        }
        if opts.axes {
            let every = ((n as f32 * 48.0 / plot.width()).ceil() as usize).max(1);
            if i % every == 0 || i == n - 1 && n <= 12 {
                painter.text(
                    pos2(bar.center().x, plot.bottom() + space::S1),
                    Align2::CENTER_TOP,
                    &b.label,
                    Type::Micro.font_id(),
                    p.text_muted,
                );
            }
        }
    }
    if resp.has_focus() {
        focus_ring(&painter, rect, CornerRadius::same(radius::SM), &theme);
    }
    resp
}

/// The table alternative: one row per bar.
pub fn bar_table(ui: &mut Ui, name: &str, bars: &[Bar]) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let id = ui.make_persistent_id(name);
    a11y::labelled(ui.ctx(), id, Role::Table, name);
    egui::Grid::new(id)
        .striped(false)
        .spacing(vec2(space::S6, space::S1))
        .show(ui, |ui| {
            for b in bars {
                ui.label(Type::Body.rich(&b.label).color(p.text_secondary));
                ui.label(Type::Body.numeric(&b.value_text).color(p.text_primary));
                ui.end_row();
            }
        });
}
