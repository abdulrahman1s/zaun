//! Path row: folder icon, **name**, location in secondary text, access segmented control,
//! remove button; full path in the tooltip (§7.4, SCREEN_SPECS_2 §9).

use egui::accesskit::Role;
use egui::{Align, Layout, Ui};

use super::icon_button::IconButton;
use super::segmented::{Segment, segmented};
use crate::a11y;
use crate::theme::tokens::{icon as icon_size, layout, space};
use crate::theme::{self, Icon, Type, icons};

/// What the user did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathRowResponse {
    /// The access choice changed.
    pub access_changed: bool,
    /// Remove was activated.
    pub removed: bool,
}

/// One granted folder. `access` indexes `access_labels` (read-only / read and change).
/// `group_name` is the accessible name of the row ("Music, in Home, can read"), `full_path`
/// the tooltip; `remove_label` names the remove button ("Remove Music").
#[allow(clippy::too_many_arguments)]
pub fn path_row(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    location: &str,
    full_path: &str,
    group_name: &str,
    access: &mut usize,
    access_labels: [&str; 2],
    remove_label: &str,
) -> PathRowResponse {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let mut out = PathRowResponse::default();
    let resp = ui
        .allocate_ui_with_layout(
            egui::vec2(ui.available_width(), layout::ROW_H),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = space::S3;
                ui.label(icons::icon(icon, icon_size::BUTTON).color(p.text_secondary));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(Type::BodyStrong.rich(name).color(p.text_primary));
                    ui.label(Type::Caption.rich(location).color(p.text_secondary));
                })
                .response
                .on_hover_text(full_path);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(IconButton::new(icons::TRASH, remove_label))
                        .clicked()
                    {
                        out.removed = true;
                    }
                    let opts = [
                        Segment {
                            icon: Some(icons::FOLDER_LOCK),
                            label: access_labels[0],
                        },
                        Segment {
                            icon: Some(icons::FOLDER),
                            label: access_labels[1],
                        },
                    ];
                    if segmented(ui, group_name, access, &opts, false).changed() {
                        out.access_changed = true;
                    }
                });
            },
        )
        .response;
    a11y::labelled(ui.ctx(), resp.id, Role::Group, group_name);
    out
}
