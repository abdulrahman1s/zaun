//! Menus: items 32 px high with an optional icon, a check mark for the chosen option and
//! shortcut hints right-aligned in `text.muted`; section labels; separators (§7.5).
//! Used inside `Popup::menu` and the per-game context menu. [`popup`] gives a menu its frame
//! and its entrance: it grows from 96 % at the corner it hangs from while egui fades it in.

use egui::{Align2, CornerRadius, Frame, Popup, Response, Sense, Ui, WidgetText, pos2, vec2};

use super::focus_ring;
use crate::a11y;
use crate::theme::tokens::{control, icon as icon_size, radius, space, travel};
use crate::theme::{self, Dur, Icon, Type, icons, motion};

/// Show `popup` with the menu frame and entrance. Use instead of `Popup::show` wherever a
/// menu or popover opens from a control.
pub fn popup<R>(popup: Popup<'_>, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    popup
        .frame(Frame::NONE)
        .gap(space::S1 + space::S0)
        .show(|ui| frame(ui, content))
        .map(|r| r.inner)
}

/// The menu frame with its entrance, inside an already open popup.
pub fn frame<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    let theme = theme::theme(ui.ctx());
    let t = motion::appear(
        ui.ctx(),
        ui.scope_id().with("menu.in"),
        theme.motion.movement(Dur::Base),
    );
    let scale = travel::POP_FROM + (1.0 - travel::POP_FROM) * t;
    // Grow from the corner nearest the control the menu hangs from.
    let pivot = ui.max_rect().left_top();
    let transform = motion::scale_about(pivot, scale, egui::Vec2::ZERO);
    ui.with_visual_transform(transform, |ui| {
        Frame::popup(ui.style()).show(ui, content).inner
    })
    .inner
}

/// One menu item. Closes the menu when clicked. `checked` shows a check mark (select lists).
pub fn item(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
    checked: bool,
) -> Response {
    item_impl(ui, icon, label, shortcut, checked, false, true)
}

/// A checklist item that keeps the menu open (filter popover).
pub fn check_item(ui: &mut Ui, label: &str, checked: bool) -> Response {
    let r = item_impl(ui, None, label, None, checked, false, false);
    a11y::option(&r, egui::Role::CheckBox, label, checked);
    r
}

/// A destructive menu item (red text).
pub fn danger_item(ui: &mut Ui, icon: Option<Icon>, label: &str) -> Response {
    item_impl(ui, icon, label, None, false, true, true)
}

fn item_impl(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
    checked: bool,
    danger: bool,
    close: bool,
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
    let lead = icon_size::INLINE + space::S2 + 2.0;
    let pad = space::S2 + 2.0;
    let min_w = pad * 2.0 + lead + g.size().x + sg.as_ref().map_or(0.0, |s| s.size().x + space::S6);
    let w = ui.available_width().max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, control::MENU_ITEM), Sense::click());
    a11y::option(&resp, egui::Role::MenuItem, label, checked);
    if let Some(s) = shortcut {
        a11y::describe(&resp, s);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let corner = CornerRadius::same(radius::ROW);
        let hot = resp.hovered() || resp.has_focus();
        if hot {
            painter.rect_filled(
                rect,
                corner,
                if danger {
                    p.danger.gamma_multiply(0.14)
                } else {
                    p.fill_selected
                },
            );
        }
        let fg = if danger { p.danger } else { p.text_primary };
        let cy = rect.center().y;
        let x = rect.left() + pad;
        let glyph = if checked { Some(icons::CHECK) } else { icon };
        if let Some(icon) = glyph {
            painter.text(
                pos2(x + icon_size::INLINE / 2.0, cy),
                Align2::CENTER_CENTER,
                icon.text(false),
                icons::font(icon_size::INLINE),
                if checked {
                    p.accent
                } else if danger {
                    p.danger
                } else if hot {
                    p.text_primary
                } else {
                    p.text_secondary
                },
            );
        }
        painter.galley(pos2(x + lead, cy - g.size().y / 2.0), g, fg);
        if let Some(sg) = sg {
            painter.galley(
                pos2(rect.right() - pad - sg.size().x, cy - sg.size().y / 2.0),
                sg,
                p.text_muted,
            );
        }
        if resp.has_focus() && !resp.hovered() {
            focus_ring(painter, rect.shrink(space::S0), corner, &theme);
        }
    }
    if resp.clicked() && close {
        ui.close();
    }
    resp
}

/// The label of a group of items ("Sort by").
pub fn section(ui: &mut Ui, title: &str) {
    let theme = theme::theme(ui.ctx());
    let h = control::SM;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + space::S2 + 2.0, rect.center().y + 1.0),
        Align2::LEFT_CENTER,
        title,
        Type::Label.font_id(),
        theme.palette.text_muted,
    );
}

/// A thin separator between menu groups.
pub fn separator(ui: &mut Ui) {
    let theme = theme::theme(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), space::S2 + 1.0), Sense::hover());
    ui.painter().hline(
        rect.shrink2(vec2(space::S2, 0.0)).x_range(),
        rect.center().y,
        egui::Stroke::new(1.0, theme.palette.hairline),
    );
}
