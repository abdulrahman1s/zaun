//! Empty state: an icon on a quiet disc, a title, one sentence, one button (§7.6). Centred
//! in the width it is given.

use egui::{Align2, Response, Sense, Ui, vec2};

use super::button::Button;
use crate::theme::tokens::{icon as icon_size, layout, space};
use crate::theme::{self, Icon, Type, icons};

/// Centred empty state. Returns the action's response when an action is given.
pub fn empty_state(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    body: &str,
    action: Option<Button<'_>>,
) -> Option<Response> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    ui.vertical_centered(|ui| {
        let w = ui.available_width().min(layout::TEXT_MAX_W * 0.8);
        ui.spacing_mut().item_spacing.y = space::S2;
        let disc = icon_size::EMPTY + space::S6;
        let (r, _) = ui.allocate_exact_size(vec2(disc, disc), Sense::hover());
        ui.painter()
            .circle_filled(r.center(), disc / 2.0, p.fill_hover);
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            icon.text(false),
            icons::font(icon_size::EMPTY - space::S4),
            p.text_muted,
        );
        ui.add_space(space::S3);
        ui.label(Type::Title.rich(title).color(p.text_primary));
        if !body.is_empty() {
            let mut job = Type::Body.job(body, p.text_secondary, w, usize::MAX);
            job.halign = egui::Align::Center;
            ui.label(job);
        }
        action.map(|b| {
            ui.add_space(space::S4);
            ui.add(b)
        })
    })
    .inner
}
