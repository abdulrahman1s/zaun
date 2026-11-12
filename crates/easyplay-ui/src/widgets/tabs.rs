//! Tab bar: text tabs with a 2 px accent underline on the selected one; arrow keys move
//! between tabs (§7.5). Horizontal scrolling when they do not fit.

use egui::accesskit::Role;
use egui::{Key, Response, ScrollArea, Sense, Stroke, Ui, WidgetText, pos2, vec2};

use super::focus_ring;
use crate::a11y;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, radius, space, stroke};
use crate::theme::{self, Type};

/// Show tabs; `selected` changes on click or arrow keys. Returns the list's response
/// (`changed()` when the selection moved).
pub fn tabs(ui: &mut Ui, label: &str, selected: &mut usize, names: &[&str]) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let before = *selected;
    let id = ui.make_persistent_id(label);
    let inner = ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::S5;
                for (i, name) in names.iter().enumerate() {
                    let g = WidgetText::from(*name).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Extend),
                        f32::INFINITY,
                        Type::BodyStrong.font_id(),
                    );
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(g.size().x, control::LG), Sense::hover());
                    let resp = ui.interact(rect, id.with(i), Sense::click());
                    super::own_arrow_keys(&resp);
                    a11y::option(&resp, Role::Tab, name, *selected == i);
                    if resp.clicked() {
                        *selected = i;
                    }
                    if resp.has_focus() {
                        let (l, r) = ui.input(|inp| {
                            (
                                inp.key_pressed(Key::ArrowLeft),
                                inp.key_pressed(Key::ArrowRight),
                            )
                        });
                        let target = if l && i > 0 {
                            Some(i - 1)
                        } else if r && i + 1 < names.len() {
                            Some(i + 1)
                        } else {
                            None
                        };
                        if let Some(t) = target {
                            *selected = t;
                            ui.memory_mut(|m| m.request_focus(id.with(t)));
                        }
                    }
                    let is_sel = *selected == i;
                    let fg = if is_sel {
                        p.text_primary
                    } else if resp.hovered() {
                        mix(p.text_secondary, p.text_primary, 0.5)
                    } else {
                        p.text_secondary
                    };
                    ui.painter().galley(
                        pos2(rect.left(), rect.center().y - g.size().y / 2.0),
                        g,
                        fg,
                    );
                    if is_sel {
                        let y = rect.bottom() - stroke::FOCUS / 2.0;
                        ui.painter()
                            .hline(rect.x_range(), y, Stroke::new(stroke::FOCUS, p.accent));
                    }
                    if resp.has_focus() {
                        focus_ring(
                            ui.painter(),
                            rect.shrink2(vec2(0.0, space::S2)),
                            egui::CornerRadius::same(radius::XS),
                            &theme,
                        );
                    }
                }
            })
        });
    let list = inner.inner.response;
    a11y::labelled(ui.ctx(), list.id, Role::TabList, label);
    // Underline across the whole bar.
    let r = list.rect;
    ui.painter().hline(
        r.x_range(),
        r.bottom(),
        Stroke::new(stroke::BORDER, p.border_subtle),
    );
    let mut list = list;
    if *selected != before {
        list.mark_changed();
    }
    list
}
