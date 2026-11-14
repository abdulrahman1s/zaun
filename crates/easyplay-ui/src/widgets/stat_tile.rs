//! Stat tile: label (`label`), value (`display`, tabular), optional caption (§7.6). And the
//! fact: the same pair without a frame, for a line of facts beside a control.

use egui::{Color32, CornerRadius, Response, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use super::bar_chart::{Bar, spark};
use crate::a11y;
use crate::theme::tokens::{layout, radius, space, stroke};
use crate::theme::{self, Type};

/// A fact in a line of facts: a label over its value ("Last played" over "Yesterday"), and
/// after the value, on its baseline, an optional strip of bars (the last seven days).
#[derive(Clone, Copy, Debug)]
pub struct Fact<'a> {
    /// The label.
    pub label: &'a str,
    /// The value, set with tabular figures.
    pub value: &'a str,
    /// The pair as one spoken phrase ("Last played: Yesterday").
    pub spoken: &'a str,
    /// Bars after the value.
    pub strip: Option<&'a [Bar]>,
    /// Colour of the value (and of the strip).
    pub fg: Color32,
    /// Colour of the label.
    pub fg2: Color32,
}

impl Fact<'_> {
    /// Height of a fact.
    pub fn height() -> f32 {
        Type::Label.line_height() + space::S0 + Type::Heading.line_height()
    }

    fn strip_width(&self) -> f32 {
        self.strip.map_or(0.0, |b| {
            let slot = layout::SPARK_BAR_W + layout::SPARK_GAP;
            space::S3 + b.len().max(1) as f32 * slot - layout::SPARK_GAP
        })
    }

    /// The width it takes, to decide whether it fits.
    pub fn width(&self, ui: &Ui) -> f32 {
        let label = Type::Label.galley(ui, self.label, self.fg2, f32::INFINITY, 1);
        let value = Type::Heading.numeric_galley(ui, self.value, self.fg);
        label
            .size()
            .x
            .max(value.size().x + self.strip_width())
            .ceil()
    }

    /// Show it.
    pub fn show(self, ui: &mut Ui) -> Response {
        let label = Type::Label.galley(ui, self.label, self.fg2, f32::INFINITY, 1);
        let value = Type::Heading.numeric_galley(ui, self.value, self.fg);
        let w = self.width(ui);
        let (rect, resp) = ui.allocate_exact_size(vec2(w, Self::height()), Sense::hover());
        a11y::item(&resp, egui::Role::Group, self.spoken);
        let value_top = rect.bottom() - value.size().y;
        let value_w = value.size().x;
        ui.painter().galley(rect.min, label, self.fg2);
        ui.painter()
            .galley(pos2(rect.left(), value_top), value, self.fg);
        if let Some(bars) = self.strip {
            // The strip stands on the value's baseline.
            let base = rect.bottom() - space::S1 - stroke::BORDER;
            let at = egui::Rect::from_min_max(
                pos2(rect.left() + value_w + space::S3, base - layout::SPARK_H),
                pos2(rect.right(), base),
            );
            ui.scope_builder(UiBuilder::new().max_rect(at), |ui| {
                spark(ui, self.label, bars, self.fg);
            });
        }
        resp
    }
}

/// A tile. `spoken` is the full-unit value for screen readers ("12 hours 5 minutes").
pub fn stat_tile(
    ui: &mut Ui,
    label: &str,
    value: &str,
    spoken: &str,
    caption: Option<&str>,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let resp = egui::Frame::NONE
        .fill(p.bg_surface)
        .stroke(Stroke::new(stroke::BORDER, p.border_subtle))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(space::S4)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = space::S1;
            ui.label(Type::Label.rich(label).color(p.text_muted));
            ui.label(Type::Display.numeric(value).color(p.text_primary));
            if let Some(c) = caption {
                ui.label(Type::Caption.rich(c).color(p.text_secondary));
            }
        })
        .response;
    a11y::labelled(
        ui.ctx(),
        resp.id,
        egui::Role::Group,
        &format!("{label}: {spoken}"),
    );
    resp
}
