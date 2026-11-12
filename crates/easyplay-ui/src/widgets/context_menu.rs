//! Menu items: 32 px high, optional icon, shortcut hints right-aligned in `text.muted` (§7.5).
//! Used inside `Response::context_menu` and `Popup::menu`.

use egui::{Align2, CornerRadius, Response, Sense, Ui, WidgetText, pos2, vec2};

use super::focus_ring;
use crate::a11y;
use crate::theme::tokens::{control, icon as icon_size, radius, space};
use crate::theme::{self, Icon, Type, icons};

/// One menu item. Closes the menu when clicked. `checked` shows a check mark (select lists).
pub fn item(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
    checked: bool,
) -> Response {
    item_impl(ui, icon, label, shortcut, checked, false)
}

/// A destructive menu item (red text).
pub fn danger_item(ui: &mut Ui, icon: Option<Icon>, label: &str) -> Response {
    item_impl(ui, icon, label, None, false, true)
}

fn item_impl(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
    checked: bool,
    danger: bool,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let g = WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        Type::Body.font_id(),
    );
    let sg = shortcut.map(|s| {
        WidgetText::from(s).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            Type::Caption.font_id(),
        )
    });
    let lead = icon_size::INLINE + space::S2;
    let min_w =
        space::S2 * 2.0 + lead + g.size().x + sg.as_ref().map_or(0.0, |s| s.size().x + space::S6);
    let w = ui.available_width().max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, control::MENU_ITEM), Sense::click());
    a11y::option(&resp, egui::Role::MenuItem, label, checked);
    if let Some(s) = shortcut {
        a11y::describe(&resp, s);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let corner = CornerRadius::same(radius::SM);
        if resp.hovered() || resp.has_focus() {
            painter.rect_filled(
                rect,
                corner,
                if danger {
                    p.danger.gamma_multiply(0.14)
                } else {
                    p.bg_overlay
                },
            );
        }
        let fg = if danger { p.danger } else { p.text_primary };
        let cy = rect.center().y;
        let x = rect.left() + space::S2;
        let glyph = if checked { Some(icons::CHECK) } else { icon };
        if let Some(icon) = glyph {
            painter.text(
                pos2(x, cy),
                Align2::LEFT_CENTER,
                icon.text(false),
                icons::font(icon_size::INLINE),
                if checked {
                    p.accent
                } else if danger {
                    p.danger
                } else {
                    p.text_secondary
                },
            );
        }
        painter.galley(pos2(x + lead, cy - g.size().y / 2.0), g, fg);
        if let Some(sg) = sg {
            painter.galley(
                pos2(
                    rect.right() - space::S2 - sg.size().x,
                    cy - sg.size().y / 2.0,
                ),
                sg,
                p.text_muted,
            );
        }
        if resp.has_focus() && !resp.hovered() {
            focus_ring(painter, rect.shrink(space::S0), corner, &theme);
        }
    }
    if resp.clicked() {
        ui.close();
    }
    resp
}

/// A thin separator between menu groups.
pub fn separator(ui: &mut Ui) {
    let theme = theme::theme(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), space::S2 + 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0, theme.palette.border_subtle),
    );
}
