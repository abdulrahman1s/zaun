//! Menus (CSS "9 · OVERLAYS" `.menu`, `.mi`, `.mh`, `.ms`; notes §5 "Menu", §6).
//!
//! A menu is a `raised` plate, radius 10, with a 1 pt `line` inner outline, the pop shadow
//! and 5 padding; 180 to 300 wide; at most min(76 % of the window, 560) high, then it
//! scrolls. Items are 28 high, radius 6, padded 8 before and 10 after, TEXT ONLY: a menu has
//! no icons (the `icon` parameters are accepted and ignored). A shortcut or count stands at
//! the right in 12 pt quiet text. Under the pointer or with focus an item takes `o2`; a
//! disabled one is `t4`; a destructive one is coral. Checkable items show an accent tick
//! (14) in a leading 14 column; the unchecked ones of a checkable group keep the column
//! empty. A section heading is 11 / 500 quiet text; a separator a 1 pt `line`, 5 above and
//! below, 4 in from the sides.
//!
//! Motion: a menu arrives from the corner it hangs from (alpha, y −4 → 0, scale .97 → 1 over
//! 140 ms, `ease_out`) and leaves over 90 ms (`ease_in`).
//!
//! Submenus replace the items in place under a back row (a 13 pt caret-left and the
//! parent's label, then a separator):
//!
//! ```ignore
//! context_menu::frame(ui, |ui| match context_menu::open_sub(ui).as_deref() {
//!     Some("tags") => {
//!         context_menu::back(ui, "Tags");
//!         // …the tag items…
//!     }
//!     _ => {
//!         context_menu::item(ui, None, "Play", Some("Ctrl+Enter"), false);
//!         context_menu::sub_item(ui, "tags", "Tags");
//!     }
//! });
//! ```
//!
//! Keys: egui moves focus with the arrows in a `PopupKind::Menu`; Home and End go to the
//! first and last item; `Esc` or Tab closes, and [`popup`] hands focus back to the control
//! that had it when the menu opened. Items that toggle (filters, sort, tags) keep the menu
//! open: show them with [`check_item`] (or [`option_item`] with `keep`) in a popup whose
//! close behaviour is `PopupCloseBehavior::CloseOnClickOutside`.

use egui::{
    Align2, Color32, CornerRadius, Frame, Id, Key, LayerId, Margin, Popup, PopupAnchor, Pos2, Rect,
    Response, ScrollArea, Sense, Stroke, Ui, pos2, vec2,
};

use super::modal::{ExitKind, track_exit};
use crate::a11y;
use crate::theme::motion::{appear, ease_out, scale_about};
use crate::theme::tokens::{
    control, icon as icon_size, layout, overlay, radius, space, stroke, travel,
};
use crate::theme::{self, Dur, Icon, Type, icons};

/// Per-menu state kept between passes: where it hangs from, which page shows, and what its
/// items were.
#[derive(Clone, Debug, Default)]
struct MenuState {
    /// The corner the menu grows from.
    pivot: Option<Pos2>,
    /// The open submenu.
    sub: Option<String>,
    /// Focus the first item of the page from the pass after this one (a submenu was
    /// entered or left in it).
    focus_first: Option<u64>,
    /// The height of the items in the last pass that did not scroll.
    content_h: f32,
    /// The last pass the menu was shown.
    pass: u64,
    /// An item of this page was checked in the last pass.
    ticks: bool,
    /// An item of this page is checked in this pass.
    ticks_now: bool,
    /// The focusable items of this pass, in order.
    items: Vec<Id>,
}

fn state_key(layer: LayerId) -> Id {
    layer.id.with("easyplay.menu")
}

fn with_state<R>(ui: &Ui, f: impl FnOnce(&mut MenuState) -> R) -> R {
    let key = state_key(ui.layer_id());
    ui.ctx()
        .data_mut(|d| f(d.get_temp_mut_or_default::<MenuState>(key)))
}

/// Show `popup` with the menu frame and motion. Use instead of `Popup::show` wherever a
/// menu opens from a control: it sets the gap to the anchor (5; 2 from a pointer position)
/// and gives focus back to the control that had it when the menu closes.
pub fn popup<R>(popup: Popup<'_>, content: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let ctx = popup.ctx().clone();
    let id = popup.get_id();
    let at_point = matches!(
        popup.get_anchor(),
        PopupAnchor::Pointer | PopupAnchor::PointerFixed | PopupAnchor::Position(_)
    );
    let gap = if at_point {
        overlay::MENU_POINTER_GAP
    } else {
        layout::MENU_GAP
    };
    let anchor = popup.get_anchor_rect();
    let opener_key = id.with("easyplay.menu.opener");
    let out = popup
        .frame(Frame::NONE)
        .gap(gap)
        .show(|ui| {
            // Remember who had focus when the menu opened, and what it hangs from.
            if ui
                .ctx()
                .data(|d| d.get_temp::<Option<Id>>(opener_key))
                .is_none()
            {
                let focused = ui.ctx().memory(|m| m.focused());
                ui.ctx().data_mut(|d| d.insert_temp(opener_key, focused));
            }
            if let Some(a) = anchor {
                let key = anchor_key(ui.layer_id());
                ui.ctx().data_mut(|d| d.insert_temp(key, a.center()));
            }
            frame(ui, content)
        })
        .map(|r| r.inner);
    if out.is_none()
        && let Some(Some(opener)) = ctx.data_mut(|d| d.remove_temp::<Option<Id>>(opener_key))
    {
        // Closed: when focus was in the menu, it goes back to the control that had it when
        // the menu opened.
        let items = ctx.data(|d| {
            d.get_temp::<MenuState>(state_key(LayerId::new(egui::Order::Foreground, id)))
                .map(|s| s.items)
                .unwrap_or_default()
        });
        let in_menu = ctx
            .memory(|m| m.focused())
            .is_none_or(|f| items.contains(&f));
        if in_menu {
            ctx.memory_mut(|m| m.request_focus(opener));
        }
    }
    out
}

fn anchor_key(layer: LayerId) -> Id {
    layer.id.with("easyplay.menu.anchor")
}

/// The menu frame with its motion, inside an already open popup.
pub fn frame<R>(ui: &mut Ui, content: impl FnOnce(&mut Ui) -> R) -> R {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let motion = theme.motion;
    let layer = ui.layer_id();
    let pass = ui.ctx().cumulative_pass_nr();
    // A menu not shown in the last pass starts afresh: its root page, no remembered pivot.
    let fresh = with_state(ui, |s| {
        let fresh = s.pass + 1 < pass;
        if fresh {
            *s = MenuState::default();
        }
        s.pass = pass;
        s.ticks = std::mem::take(&mut s.ticks_now) || (s.ticks && fresh);
        s.items.clear();
        fresh
    });
    let alpha = appear(ui.ctx(), layer.id.with("menu.in"), motion.fade(Dur::Fast));
    let t = ease_out(appear(
        ui.ctx(),
        layer.id.with("menu.move"),
        motion.movement(Dur::Fast),
    ));
    let top_left = ui.max_rect().left_top();
    // Grow from the corner nearest what the menu hangs from: its anchor, or where the
    // pointer was when it opened.
    let anchor: Option<Pos2> = ui.ctx().data(|d| d.get_temp(anchor_key(layer)));
    let pointer = ui.ctx().input(|i| i.pointer.interact_pos());
    let reference = with_state(ui, |s| {
        if fresh || s.pivot.is_none() {
            s.pivot = anchor.or(pointer);
        }
        s.pivot
    });
    let last_rect = ui
        .ctx()
        .read_response(layer.id)
        .map(|r| r.rect)
        .unwrap_or(Rect::from_min_size(top_left, vec2(layout::MENU_W[0], 0.0)));
    let pivot = reference.map_or(last_rect.left_top(), |r| nearest_corner(last_rect, r));
    let transform = scale_about(
        pivot,
        1.0 - (1.0 - travel::POP_FROM) * (1.0 - t),
        vec2(0.0, -travel::MENU_DROP * (1.0 - t)),
    );
    let max_h =
        (ui.ctx().content_rect().height() * overlay::MENU_MAX_H_FRACTION).min(layout::MENU_MAX_H);
    let out = ui
        .with_visual_transform(transform, |ui| {
            ui.multiply_opacity(alpha);
            Frame::NONE
                .fill(p.raised)
                .stroke(Stroke::new(stroke::BORDER, p.line))
                .corner_radius(CornerRadius::same(radius::POP))
                .shadow(p.shade_pop.shadow())
                .inner_margin(Margin::same(layout::MENU_PAD as i8))
                .show(ui, |ui| {
                    let pad = layout::MENU_PAD * 2.0;
                    ui.set_min_width(layout::MENU_W[0] - pad);
                    ui.set_max_width(layout::MENU_W[1] - pad);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    // A menu scrolls only when its items do not fit; otherwise it is as
                    // tall as they are (a submenu taller than its parent grows at once).
                    let tall = with_state(ui, |s| s.content_h) > max_h - pad;
                    let top = ui.cursor().min.y;
                    let out = if tall {
                        ScrollArea::vertical()
                            .id_salt("menu.scroll")
                            .max_height(max_h - pad)
                            .auto_shrink([true, true])
                            .show(ui, content)
                            .inner
                    } else {
                        content(ui)
                    };
                    let h = ui.min_rect().bottom() - top;
                    if !tall {
                        with_state(ui, |s| s.content_h = h);
                    }
                    out
                })
                .inner
        })
        .inner;
    keys(ui);
    track_exit(
        ui.ctx(),
        layer.id.with("menu.exit"),
        layer,
        ExitKind::Menu { pivot },
    );
    out
}

fn nearest_corner(r: Rect, p: Pos2) -> Pos2 {
    pos2(
        if (p.x - r.left()).abs() <= (p.x - r.right()).abs() {
            r.left()
        } else {
            r.right()
        },
        if (p.y - r.top()).abs() <= (p.y - r.bottom()).abs() {
            r.top()
        } else {
            r.bottom()
        },
    )
}

/// Home and End move focus to the first and last item; Tab closes.
fn keys(ui: &mut Ui) {
    let items = with_state(ui, |s| s.items.clone());
    let focused = ui.ctx().memory(|m| m.focused());
    if !focused.is_some_and(|f| items.contains(&f)) {
        return;
    }
    let (home, end, tab) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::Home),
            i.consume_key(egui::Modifiers::NONE, Key::End),
            i.key_pressed(Key::Tab),
        )
    });
    if home && let Some(first) = items.first() {
        ui.ctx().memory_mut(|m| m.request_focus(*first));
    }
    if end && let Some(last) = items.last() {
        ui.ctx().memory_mut(|m| m.request_focus(*last));
    }
    if tab {
        ui.close();
    }
}

/// Put focus on the first item of the menu `ui` belongs to from the next pass: a menu
/// opened from the keyboard (call it in the menu's first pass).
pub fn focus_first(ui: &Ui) {
    let pass = ui.ctx().cumulative_pass_nr();
    with_state(ui, |s| s.focus_first = Some(pass));
    ui.ctx().request_repaint();
}

/// The open submenu of the menu `ui` belongs to, if any.
pub fn open_sub(ui: &Ui) -> Option<String> {
    with_state(ui, |s| s.sub.clone())
}

/// An item that opens the submenu `key` in place: the label, a 13 pt caret at its right.
/// Clicking it replaces the menu's items by the submenu's (see [`open_sub`]).
pub fn sub_item(ui: &mut Ui, key: &str, label: &str) -> Response {
    let r = item_impl(ui, label, None, Look::plain(), Trail::Caret, false);
    a11y::item(&r, egui::Role::MenuItem, label);
    // The right arrow enters a submenu as a click does.
    let right = r.has_focus() && ui.input(|i| i.key_pressed(Key::ArrowRight));
    if r.clicked() || right {
        let key = key.to_owned();
        let pass = ui.ctx().cumulative_pass_nr();
        with_state(ui, |s| {
            s.sub = Some(key);
            s.focus_first = Some(pass);
            s.ticks = false;
            s.ticks_now = false;
        });
        ui.ctx().request_repaint();
    }
    r
}

/// The back row of an open submenu: a 13 pt caret-left and the parent's label, then a
/// separator. Clicking it (or the left arrow) shows the parent's items again.
pub fn back(ui: &mut Ui, label: &str) -> Response {
    let r = item_impl(ui, label, None, Look::plain(), Trail::Back, false);
    a11y::item(&r, egui::Role::MenuItem, label);
    let left = r.has_focus() && ui.input(|i| i.key_pressed(Key::ArrowLeft));
    if r.clicked() || left {
        let pass = ui.ctx().cumulative_pass_nr();
        with_state(ui, |s| {
            s.sub = None;
            s.focus_first = Some(pass);
            s.ticks = false;
            s.ticks_now = false;
        });
        ui.ctx().request_repaint();
    }
    separator(ui);
    r
}

/// One menu item. Closes the menu when clicked. `checked` shows the accent tick; the
/// unchecked items of a menu where something is checked keep the tick's column empty.
/// `icon` is not drawn: menus have no icons.
pub fn item(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
    checked: bool,
) -> Response {
    let _ = icon;
    let look = Look {
        column: checked || with_state(ui, |s| s.ticks),
        checked,
        danger: false,
        dot: None,
    };
    let r = item_impl(ui, label, shortcut, look, Trail::None, true);
    a11y::option(&r, egui::Role::MenuItem, label, checked);
    r
}

/// One of a checkable group (sort by, order, group by, a protection level): the tick's
/// column is always kept. Closes the menu when clicked.
pub fn option_item(ui: &mut Ui, label: &str, checked: bool) -> Response {
    let look = Look {
        column: true,
        checked,
        danger: false,
        dot: None,
    };
    let r = item_impl(ui, label, None, look, Trail::None, true);
    a11y::option(&r, egui::Role::MenuItemRadio, label, checked);
    r
}

/// A checklist item that keeps the menu open (filters, tags). `count` is written at the
/// right in quiet text.
pub fn check_item_with(ui: &mut Ui, label: &str, checked: bool, count: Option<&str>) -> Response {
    let look = Look {
        column: true,
        checked,
        danger: false,
        dot: None,
    };
    let r = item_impl(ui, label, count, look, Trail::None, false);
    a11y::option(&r, egui::Role::MenuItemCheckBox, label, checked);
    r
}

/// A checklist item that keeps the menu open (filters, tags).
pub fn check_item(ui: &mut Ui, label: &str, checked: bool) -> Response {
    check_item_with(ui, label, checked, None)
}

/// A tag in a menu that ticks and unticks tags: the tag's 7 pt `dot`, its name, the tick
/// while `checked`. Keeps the menu open.
pub fn tag_item(ui: &mut Ui, label: &str, dot: Color32, checked: bool) -> Response {
    let look = Look {
        column: true,
        checked,
        danger: false,
        dot: Some(dot),
    };
    let r = item_impl(ui, label, None, look, Trail::None, false);
    a11y::option(&r, egui::Role::MenuItemCheckBox, label, checked);
    r
}

/// A destructive menu item (coral text). `icon` is not drawn.
pub fn danger_item(ui: &mut Ui, icon: Option<Icon>, label: &str) -> Response {
    danger_item_with(ui, icon, label, None)
}

/// A destructive menu item with its shortcut ("Remove…  Del"). `icon` is not drawn.
pub fn danger_item_with(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
) -> Response {
    let _ = icon;
    let look = Look {
        column: with_state(ui, |s| s.ticks),
        checked: false,
        danger: true,
        dot: None,
    };
    let r = item_impl(ui, label, shortcut, look, Trail::None, true);
    a11y::item(&r, egui::Role::MenuItem, label);
    r
}

/// An item that cannot be chosen here (`t4`), with the reason beneath it in quiet text.
pub fn unavailable_item(ui: &mut Ui, label: &str, why: &str) {
    ui.add_enabled_ui(false, |ui| {
        let r = item_impl(ui, label, None, Look::plain(), Trail::None, false);
        a11y::item(&r, egui::Role::MenuItem, label);
        a11y::describe(&r, why);
    });
    let p = theme::theme(ui.ctx()).palette.clone();
    let [pad, end] = overlay::MENU_ITEM_PAD;
    let w = ui.available_width() - pad - end;
    let g = Type::Caption.galley(ui, why, p.t3, w.max(1.0), usize::MAX);
    let (r, why_resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), g.size().y + overlay::MENU_HEAD_PAD[2]),
        Sense::hover(),
    );
    ui.painter().galley(pos2(r.left() + pad, r.top()), g, p.t3);
    a11y::item(&why_resp, egui::Role::Label, why);
}

#[derive(Clone, Copy)]
struct Look {
    column: bool,
    checked: bool,
    danger: bool,
    /// A tag's 7 pt dot before the label.
    dot: Option<Color32>,
}

impl Look {
    const fn plain() -> Self {
        Self {
            column: false,
            checked: false,
            danger: false,
            dot: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Trail {
    None,
    /// A submenu: a caret at the right.
    Caret,
    /// The back row: a caret-left before the label.
    Back,
}

/// "Ctrl+Enter" as the design writes it in a menu: "Ctrl Enter".
fn spaced(s: &str) -> String {
    if s.len() <= 1 {
        s.to_owned()
    } else {
        s.split('+')
            .filter(|k| !k.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn item_impl(
    ui: &mut Ui,
    label: &str,
    shortcut: Option<&str>,
    look: Look,
    trail: Trail,
    close: bool,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let [pad, end] = overlay::MENU_ITEM_PAD;
    let gap = overlay::MENU_ITEM_GAP;
    let enabled = ui.is_enabled();
    let fg = if !enabled {
        p.t4
    } else if look.danger {
        p.coral
    } else {
        p.t1
    };
    let lead = if look.column || trail == Trail::Back {
        overlay::MENU_TICK.max(icon_size::SMALL) + gap
    } else {
        0.0
    };
    let dot_w = look.dot.map_or(0.0, |_| control::DOT + space::S1);
    let sg = shortcut.map(|s| Type::Caption.line(ui, &spaced(s), p.t3));
    let tail = match (&sg, trail) {
        (Some(g), _) => g.size().x + gap,
        (None, Trail::Caret) => icon_size::SMALL + gap,
        _ => 0.0,
    };
    let full = Type::Body.line(ui, label, fg);
    let min_w = pad + lead + dot_w + full.size().x + tail + end;
    let w = ui
        .available_width()
        .max(min_w.min(layout::MENU_W[1] - layout::MENU_PAD * 2.0));
    let (rect, resp) = ui.allocate_exact_size(vec2(w, control::MENU_ITEM), Sense::click());
    if enabled {
        // Items shown outside [`frame`] (a dialog's) keep only this pass's list too.
        let pass = ui.ctx().cumulative_pass_nr();
        with_state(ui, |s| {
            if s.pass != pass {
                s.items.clear();
                s.pass = pass;
            }
            s.items.push(resp.id);
        });
    }
    if look.checked {
        with_state(ui, |s| s.ticks_now = true);
    }
    // Entering or leaving a submenu puts focus on the page's first item.
    let pass_now = ui.ctx().cumulative_pass_nr();
    let take_focus = enabled
        && with_state(ui, |s| match s.focus_first {
            Some(set) if set < pass_now => {
                s.focus_first = None;
                true
            }
            _ => false,
        });
    if take_focus {
        resp.request_focus();
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let corner = CornerRadius::same(radius::SM);
        let hot = enabled && (resp.hovered() || resp.has_focus());
        if hot {
            painter.rect_filled(rect, corner, p.o2);
        }
        let cy = rect.center().y;
        let x = rect.left() + pad;
        if look.checked {
            painter.text(
                pos2(x + overlay::MENU_TICK / 2.0, cy),
                Align2::CENTER_CENTER,
                icons::CHECK.text(false),
                icons::font(overlay::MENU_TICK),
                p.accent_mark,
            );
        }
        if trail == Trail::Back {
            painter.text(
                pos2(x + icon_size::SMALL / 2.0, cy),
                Align2::CENTER_CENTER,
                icons::CARET_LEFT.text(false),
                icons::font(icon_size::SMALL),
                fg,
            );
        }
        if let Some(c) = look.dot {
            painter.circle_filled(
                pos2(x + lead + control::DOT / 2.0, cy),
                control::DOT / 2.0,
                c,
            );
        }
        let lx = x + lead + dot_w;
        let label_w = (rect.right() - end - tail - lx).max(1.0);
        let g = if full.size().x > label_w {
            Type::Body.galley(ui, label, fg, label_w, 1)
        } else {
            full
        };
        painter.galley(pos2(lx, cy - g.size().y / 2.0), g, fg);
        if let Some(sg) = sg {
            painter.galley(
                pos2(rect.right() - end - sg.size().x, cy - sg.size().y / 2.0),
                sg,
                p.t3,
            );
        }
        if trail == Trail::Caret {
            painter.text(
                pos2(rect.right() - end - icon_size::SMALL / 2.0, cy),
                Align2::CENTER_CENTER,
                icons::CARET_RIGHT.text(false),
                icons::font(icon_size::SMALL),
                p.t3,
            );
        }
    }
    if resp.clicked() && close {
        ui.close();
    }
    if let Some(s) = shortcut {
        a11y::describe(&resp, s);
    }
    resp
}

/// The heading of a group of items ("Sort by"): 11 / 500 quiet text, padded 7 above, 8 at
/// the sides and 3 below.
pub fn section(ui: &mut Ui, title: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let [top, side, bottom] = overlay::MENU_HEAD_PAD;
    let g = Type::MicroStrong.line(ui, title, p.t3);
    let h = top + Type::MicroStrong.line_height() + bottom;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    ui.painter()
        .galley(pos2(rect.left() + side, rect.top() + top), g, p.t3);
    a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, title);
}

/// A separator between groups: a 1 pt `line`, 5 above and below, 4 in from the sides.
pub fn separator(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let [v, side] = overlay::MENU_RULE;
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), v * 2.0 + stroke::BORDER),
        Sense::hover(),
    );
    ui.painter().rect_filled(
        Rect::from_min_size(
            pos2(rect.left() + side, rect.top() + v),
            vec2(rect.width() - side * 2.0, stroke::BORDER),
        ),
        0.0,
        p.line,
    );
}
