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
    /// Remove was activated (the button, or `Delete` while focus is in the row).
    pub removed: bool,
    /// *Locate…* was activated (missing folders only).
    pub locate: bool,
}

/// How a row is shown beyond name and access.
#[derive(Clone, Copy, Debug, Default)]
pub struct PathRowState<'a> {
    /// The folder does not exist: "Not found" and *Locate…* (SCREEN_SPECS_2 §9 "Error").
    pub missing: Option<(&'a str, &'a str)>,
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
    path_row_with(
        ui,
        icon,
        name,
        location,
        full_path,
        group_name,
        access,
        access_labels,
        remove_label,
        PathRowState::default(),
    )
}

/// [`path_row`] with a [`PathRowState`]: `missing = Some((not_found, locate))` labels.
#[allow(clippy::too_many_arguments)]
pub fn path_row_with(
    ui: &mut Ui,
    icon: Icon,
    name: &str,
    location: &str,
    full_path: &str,
    group_name: &str,
    access: &mut usize,
    access_labels: [&str; 2],
    remove_label: &str,
    row: PathRowState<'_>,
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
                    match row.missing {
                        Some((not_found, _)) => {
                            ui.label(Type::Caption.rich(not_found).color(p.danger));
                        }
                        None => {
                            ui.label(Type::Caption.rich(location).color(p.text_secondary));
                        }
                    }
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
                    if let Some((_, locate)) = row.missing
                        && ui
                            .add(
                                crate::widgets::button::Button::ghost(locate)
                                    .size(crate::widgets::button::Size::Sm),
                            )
                            .clicked()
                    {
                        out.locate = true;
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
    // `Delete` removes the row whose control has keyboard focus.
    let focus_inside = ui
        .ctx()
        .memory(|m| m.focused())
        .and_then(|f| ui.ctx().read_response(f))
        .is_some_and(|r| resp.rect.contains_rect(r.rect));
    if focus_inside && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
        out.removed = true;
    }
    out
}
