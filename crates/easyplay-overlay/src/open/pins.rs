//! "Show while playing" (`chat-calls.html` §G): a popover 300 wide under the pin, raised,
//! radius 10, a 1-point line2. A heading, then rows 38 high that toggle as a whole: "Show new
//! messages", "Show people in the call", "Corner" with a menu of four (dimmed to 38 % while
//! the people are hidden). These are the Settings switches: one setting, two places.
use easyplay_core::chat::overlay::{Corner, FromOverlay};
use easyplay_ui::theme::{Type, icons};
use egui::{
    Align, Area, Context, CornerRadius, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

use super::layout::Row;
use crate::look::Look;
use crate::model::Model;
use crate::paint::{icon, text};
use crate::words;

const WIDTH: f32 = 300.0;
const ROW: f32 = 38.0;
const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomLeft,
    Corner::BottomRight,
];

/// The popover with its start at `anchor` (the pin's bottom start corner).
pub fn draw(ctx: &Context, m: &mut Model, anchor: egui::Pos2) {
    let rtl = words::rtl();
    let x = if rtl { anchor.x - WIDTH } else { anchor.x };
    let height = 6.0 + 26.0 + 3.0 * ROW + 6.0;
    let rect = Rect::from_min_size(pos2(x, anchor.y + 4.0), vec2(WIDTH, height));
    Area::new(Id::unique("ovl-pins"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            let look = m.look;
            let p = ui.painter();
            p.rect_filled(rect, CornerRadius::same(10), look.raised);
            p.rect_stroke(
                rect,
                CornerRadius::same(10),
                Stroke::new(1.0, look.line2),
                StrokeKind::Inside,
            );
            let inner = Row::new(rect.shrink(6.0), rtl);
            text::line(
                ui,
                Type::MicroStrong,
                words::text("overlay.show_while_playing"),
                look.t3,
                pos2(inner.x(8.0), rect.top() + 12.0),
                WIDTH,
                rtl,
            );
            let (on_toasts, on_widget, on_corner) = (
                m.snap.look.toasts,
                m.snap.look.voice_widget,
                m.snap.look.corner,
            );
            let mut y = rect.top() + 6.0 + 26.0;
            let toasts = switch_row(ui, m, &inner, y, "chat.settings.toasts", on_toasts, true);
            y += ROW;
            let widget = switch_row(ui, m, &inner, y, "chat.settings.widget", on_widget, true);
            y += ROW;
            // The corner places the toasts and the call widget: dim only with both off.
            let corner = corner_row(ctx, ui, m, &inner, y, on_corner, toasts || widget);
            if toasts != on_toasts || widget != on_widget || corner != on_corner {
                m.said.push(FromOverlay::SetLook {
                    toasts,
                    voice_widget: widget,
                    corner,
                });
                let look = &mut m.snap_mut().look;
                look.toasts = toasts;
                look.voice_widget = widget;
                look.corner = corner;
            }
        });
}

fn switch_row(
    ui: &mut Ui,
    m: &Model,
    row: &Row,
    y: f32,
    key: &str,
    on: bool,
    enabled: bool,
) -> bool {
    let look = &m.look;
    let rect = Rect::from_min_size(pos2(row.rect.left(), y), vec2(row.rect.width(), ROW));
    let r = ui.interact(
        rect,
        Id::unique(("ovl-pin-row", key)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if r.hovered() || r.has_focus() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(7), look.o1);
    }
    let a = if enabled { 1.0 } else { 0.38 };
    text::line(
        ui,
        Type::Body,
        words::text(key),
        Look::alpha(look.t1, a),
        pos2(row.x(8.0), rect.center().y - 9.0),
        220.0,
        row.rtl,
    );
    let sw = Row::new(rect, row.rtl).end_span(8.0, 32.0, rect.center().y - 9.0, 18.0);
    let on_now = if r.clicked() { !on } else { on };
    let track = if on_now { look.fill } else { look.o3 };
    ui.painter()
        .rect_filled(sw, CornerRadius::same(9), Look::alpha(track, a));
    let knob_x = if on_now != row.rtl {
        sw.right() - 9.0
    } else {
        sw.left() + 9.0
    };
    ui.painter().circle_filled(
        pos2(knob_x, sw.center().y),
        6.5,
        Look::alpha(if on_now { look.on_fill } else { look.t2 }, a),
    );
    on_now
}

fn corner_row(
    ctx: &Context,
    ui: &mut Ui,
    m: &mut Model,
    row: &Row,
    y: f32,
    corner: Corner,
    enabled: bool,
) -> Corner {
    let look = m.look;
    let rect = Rect::from_min_size(pos2(row.rect.left(), y), vec2(row.rect.width(), ROW));
    let a = if enabled { 1.0 } else { 0.38 };
    text::line(
        ui,
        Type::Body,
        words::text("chat.settings.corner"),
        Look::alpha(look.t1, a),
        pos2(row.x(8.0), rect.center().y - 9.0),
        150.0,
        row.rtl,
    );
    let label = words::corner(corner);
    let g = text::galley(ui, Type::Body, label, Look::alpha(look.t1, a), 140.0, 1);
    let w = g.size().x + 10.0 + 16.0 + 10.0;
    let sel = Row::new(rect, row.rtl).end_span(8.0, w, rect.center().y - 13.0, 26.0);
    let r = ui.interact(
        sel,
        Id::unique("ovl-corner"),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    ui.painter().rect_filled(
        sel,
        CornerRadius::same(6),
        Look::alpha(if r.hovered() { look.o3 } else { look.o2 }, a),
    );
    let srow = Row::new(sel, row.rtl);
    text::put(
        ui,
        &g,
        pos2(srow.x(10.0), sel.center().y),
        srow.start_anchor(Align::Center),
    );
    icon::icon(
        ui.painter(),
        pos2(srow.x(w - 14.0), sel.center().y),
        icons::CARET_DOWN,
        12.0,
        Look::alpha(look.t2, a),
    );
    if r.clicked() {
        m.corner_menu = !m.corner_menu;
    }
    let mut picked = corner;
    if m.corner_menu && enabled {
        Area::new(Id::unique("ovl-corner-menu"))
            .order(Order::Tooltip)
            .fixed_pos(pos2(sel.left(), sel.bottom() + 4.0))
            .show(ctx, |ui| {
                let menu = Rect::from_min_size(
                    pos2(sel.left(), sel.bottom() + 4.0),
                    vec2(170.0, 4.0 * 30.0 + 8.0),
                );
                ui.set_min_size(menu.size());
                ui.painter()
                    .rect_filled(menu, CornerRadius::same(8), look.raised);
                ui.painter().rect_stroke(
                    menu,
                    CornerRadius::same(8),
                    Stroke::new(1.0, look.line2),
                    StrokeKind::Inside,
                );
                for (i, c) in CORNERS.iter().enumerate() {
                    let item = Rect::from_min_size(
                        pos2(menu.left() + 4.0, menu.top() + 4.0 + i as f32 * 30.0),
                        vec2(menu.width() - 8.0, 30.0),
                    );
                    let ir = ui.interact(item, Id::unique(("ovl-corner-item", i)), Sense::click());
                    if ir.hovered() {
                        ui.painter()
                            .rect_filled(item, CornerRadius::same(6), look.o2);
                    }
                    let irow = Row::new(item, row.rtl);
                    if *c == corner {
                        icon::icon(
                            ui.painter(),
                            pos2(irow.x(14.0), item.center().y),
                            icons::CHECK,
                            12.0,
                            look.atext,
                        );
                    }
                    text::line(
                        ui,
                        Type::Body,
                        words::corner(*c),
                        look.t1,
                        pos2(irow.x(28.0), item.center().y - 9.0),
                        130.0,
                        row.rtl,
                    );
                    if ir.clicked() {
                        picked = *c;
                        m.corner_menu = false;
                    }
                }
            });
    }
    picked
}
