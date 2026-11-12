//! Empty state: 48 px icon in `text.muted`, `heading`, one sentence, one button (§7.6).

use egui::{Response, Ui};

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
    let p = &theme.palette;
    ui.vertical_centered(|ui| {
        ui.set_max_width(layout::TEXT_MAX_W);
        ui.spacing_mut().item_spacing.y = space::S2;
        ui.label(icons::icon(icon, icon_size::EMPTY).color(p.text_muted));
        ui.add_space(space::S2);
        ui.label(Type::Heading.rich(title).color(p.text_primary));
        ui.label(Type::Body.rich(body).color(p.text_secondary));
        action.map(|b| {
            ui.add_space(space::S3);
            ui.add(b)
        })
    })
    .inner
}
