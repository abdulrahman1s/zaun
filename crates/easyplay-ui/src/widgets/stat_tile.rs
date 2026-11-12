//! Stat tile: label (`micro`), value (`display`, tabular), optional caption (§7.6).

use egui::{CornerRadius, Response, Stroke, Ui};

use crate::a11y;
use crate::theme::tokens::{radius, space, stroke};
use crate::theme::{self, Type};

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
            ui.label(Type::Micro.rich(label).color(p.text_muted));
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
