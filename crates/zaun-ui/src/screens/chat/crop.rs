//! The crop step of a picture (ADR-0038; chat-calls.html C "CROP STEP"): the picture in a
//! black area as wide as the dialog and 280 high, under a 240 circle; outside the circle a
//! 56 % scrim, its edge a 1.5 white line at 72 %. At zoom 1 the picture just covers the area
//! and it can never be moved off an edge. Drag moves it; the area is one tab stop where the
//! arrows move it 8 (Shift: 32) and + / − or the wheel zoom 10 % about the circle's centre. A
//! zoom row lies 14 under it: a 13 glyph, the slider (100–400 %), a 16 glyph.

use egui::epaint::{Brush, Mesh, RectShape, Vertex, WHITE_UV};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2,
    pos2, vec2,
};

use crate::strings::chat::picture as s;
use crate::theme::{self, icons};
use crate::vm::chat_picture::{Placed, ZOOM};

mod m {
    pub const AREA_H: f32 = 280.0;
    pub const RADIUS: f32 = 10.0;
    /// The circle's diameter.
    pub const CIRCLE: f32 = 240.0;
    /// The scrim's alpha (56 %), the edge's width and alpha (72 %).
    pub const SCRIM: u8 = 143;
    pub const EDGE: [f32; 2] = [1.5, 0.72];
    pub const ZOOM_ABOVE: f32 = 14.0;
    /// The zoom row's glyphs and the gaps beside them.
    pub const GLYPHS: [f32; 2] = [13.0, 16.0];
    pub const GLYPH_GAP: f32 = 10.0;
    /// Arrow steps, plain and with Shift (points).
    pub const STEP: [f32; 2] = [8.0, 32.0];
    /// One zoom step (+, −, a wheel notch).
    pub const ZOOM_STEP: f32 = 1.1;
    /// A wheel notch in points (a touchpad's unit), and a page in notches.
    pub const POINTS_PER_NOTCH: f32 = 50.0;
    pub const NOTCHES_PER_PAGE: f32 = 5.0;
    /// Points between the scrim's outline samples.
    pub const SAMPLES: usize = 96;
}

/// The circle's diameter in points: the crop the core gets is this circle's square.
pub const CIRCLE: f32 = m::CIRCLE;

/// Draw the crop step's body and move `placed` as the user drags, zooms and presses keys.
pub fn show(ui: &mut Ui, placed: &mut Placed) {
    // The dialog puts its body 16 under the sentence, as the design's area wants.
    let w = ui.available_width();
    let (area, resp) = ui.allocate_exact_size(vec2(w, m::AREA_H), Sense::click_and_drag());
    crate::a11y::item(&resp, egui::Role::Image, s::CROP_AREA());
    let size = placed.size();
    placed.area = [area.width(), area.height()];
    let view = &mut placed.view;
    let scale = view.scale(size, placed.area);
    let mut pan = -resp.drag_delta();
    let mut zoom = 1.0;
    if resp.has_focus() {
        crate::widgets::own_arrow_keys(&resp);
        ui.input_mut(|i| {
            // Shift first: a plain pattern would also take Shift+arrow.
            for (mods, step) in [
                (Modifiers::SHIFT, m::STEP[1]),
                (Modifiers::NONE, m::STEP[0]),
            ] {
                // An arrow moves the picture, so the point under the circle goes the other way.
                for (key, d) in [
                    (Key::ArrowLeft, vec2(1.0, 0.0)),
                    (Key::ArrowRight, vec2(-1.0, 0.0)),
                    (Key::ArrowUp, vec2(0.0, 1.0)),
                    (Key::ArrowDown, vec2(0.0, -1.0)),
                ] {
                    if i.consume_key(mods, key) {
                        pan += d * step;
                    }
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Plus)
                || i.consume_key(Modifiers::NONE, Key::Equals)
            {
                zoom *= m::ZOOM_STEP;
            }
            if i.consume_key(Modifiers::NONE, Key::Minus) {
                zoom /= m::ZOOM_STEP;
            }
        });
    }
    if resp.hovered() {
        let wheel = ui.input_mut(|i| {
            let d: f32 = i
                .events
                .iter()
                .filter_map(|e| match e {
                    // In notches: a touchpad's points add up to steps as a wheel's do.
                    egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                        egui::MouseWheelUnit::Point => delta.y / m::POINTS_PER_NOTCH,
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * m::NOTCHES_PER_PAGE,
                    }),
                    _ => None,
                })
                .sum();
            // The wheel zooms here; the dialog's body does not scroll under it.
            i.smooth_scroll_delta = Vec2::ZERO;
            d
        });
        zoom *= m::ZOOM_STEP.powf(wheel);
        let icon = if resp.dragged() {
            CursorIcon::Grabbing
        } else {
            CursorIcon::Grab
        };
        ui.ctx().set_cursor_icon(icon);
    }
    view.centre[0] += pan.x / scale;
    view.centre[1] += pan.y / scale;
    view.zoom *= zoom;
    view.clamp(size, placed.area);
    paint(ui, area, placed, &resp);
    zoom_row(ui, placed);
}

fn paint(ui: &Ui, area: Rect, placed: &Placed, resp: &egui::Response) {
    let theme = theme::theme(ui.ctx());
    let painter = ui.painter();
    let corner = CornerRadius::same(m::RADIUS as u8);
    let size = placed.size();
    let s = placed.view.scale(size, placed.area);
    let img = Rect::from_min_size(
        area.center() - vec2(placed.view.centre[0], placed.view.centre[1]) * s,
        vec2(size[0], size[1]) * s,
    );
    painter.rect_filled(area, corner, Color32::BLACK);
    match super::pictures::whole(ui.ctx(), &placed.source.preview) {
        Some(tex) => {
            // Only the part of the picture inside the area, so its corners stay round.
            let uv = Rect::from_min_max(
                ((area.min - img.min) / img.size()).to_pos2(),
                ((area.max - img.min) / img.size()).to_pos2(),
            );
            let mut shape = RectShape::filled(area, corner, Color32::WHITE);
            shape.brush = Some(std::sync::Arc::new(Brush {
                fill_texture_id: tex,
                uv,
            }));
            painter.add(shape);
        }
        None => crate::widgets::progress::spinner(ui, area.center(), 8.0, Color32::WHITE),
    }
    painter.add(scrim(area, m::CIRCLE / 2.0));
    painter.circle_stroke(
        area.center(),
        m::CIRCLE / 2.0,
        Stroke::new(m::EDGE[0], Color32::WHITE.gamma_multiply(m::EDGE[1])),
    );
    if resp.has_focus() {
        crate::widgets::focus_ring(painter, area, corner, &theme);
    }
}

/// The scrim over `area` (rounded 10) with a hole of `r` at its centre: a strip of
/// quadrilaterals between the circle and the area's outline along the same rays.
fn scrim(area: Rect, r: f32) -> Mesh {
    let c = area.center();
    let colour = Color32::from_black_alpha(m::SCRIM);
    let mut mesh = Mesh::default();
    for i in 0..m::SAMPLES {
        let a = i as f32 / m::SAMPLES as f32 * std::f32::consts::TAU;
        let dir = vec2(a.cos(), a.sin());
        for p in [c + dir * r, c + dir * outline(area, dir)] {
            mesh.vertices.push(Vertex {
                pos: p,
                uv: WHITE_UV,
                color: colour,
            });
        }
        let (k, n) = (i as u32 * 2, ((i + 1) % m::SAMPLES) as u32 * 2);
        mesh.indices
            .extend_from_slice(&[k, k + 1, n + 1, k, n + 1, n]);
    }
    mesh
}

/// How far from `area`'s centre its rounded outline lies along `dir` (a unit vector).
fn outline(area: Rect, dir: Vec2) -> f32 {
    let half = area.size() / 2.0;
    let t = (half.x / dir.x.abs().max(1e-6)).min(half.y / dir.y.abs().max(1e-6));
    let hit = dir * t;
    let inner = half - Vec2::splat(m::RADIUS);
    if hit.x.abs() <= inner.x || hit.y.abs() <= inner.y {
        return t;
    }
    // In a corner: where the ray leaves the corner's circle.
    let k = vec2(inner.x * dir.x.signum(), inner.y * dir.y.signum());
    let b = dir.dot(k);
    let disc = b * b - (k.length_sq() - m::RADIUS * m::RADIUS);
    b + disc.max(0.0).sqrt()
}

/// The zoom row: a small picture glyph, the slider, a large one.
fn zoom_row(ui: &mut Ui, placed: &mut Placed) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(m::ZOOM_ABOVE);
    let w = ui.available_width();
    let (row, _) =
        ui.allocate_exact_size(vec2(w, theme::tokens::control::SLIDER_H), Sense::hover());
    let [small, large] = m::GLYPHS;
    // Small to large in the slider's direction (it is not mirrored in a right-to-left
    // interface).
    let (lead, trail) = (row.left() + small / 2.0, row.right() - large / 2.0);
    for (x, size) in [(lead, small), (trail, large)] {
        ui.painter().text(
            pos2(x, row.center().y),
            Align2::CENTER_CENTER,
            icons::IMAGE.text(false),
            icons::font(size),
            p.t3,
        );
    }
    let slider = Rect::from_min_max(
        Pos2::new(row.left() + small + m::GLYPH_GAP, row.top()),
        Pos2::new(row.right() - large - m::GLYPH_GAP, row.bottom()),
    );
    let mut percent = placed.view.zoom * 100.0;
    let value = s::zoom_value(percent.round() as u32);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(slider));
    let resp = crate::widgets::slider::slider(
        &mut child,
        &mut percent,
        ZOOM[0] * 100.0..=ZOOM[1] * 100.0,
        1.0,
        slider.width(),
        s::ZOOM(),
        &value,
    );
    if resp.changed() {
        placed.view.zoom = percent / 100.0;
        placed.view.clamp(placed.size(), placed.area);
    }
}
