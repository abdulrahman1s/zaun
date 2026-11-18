//! A watched game's picture (ADR-0036): uploaded from the frame slot into one reused
//! texture, letterboxed on black, dimmed while paused or ended; a still stand-in until a
//! frame arrives.

use egui::{Color32, CornerRadius, Id, Painter, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};

use crate::theme::palette::{self, fade, mix};

/// The picture's corner.
pub const RADIUS: u8 = 12;

/// How a paused or ended picture is dimmed: the colour kept (saturation) and the light
/// kept, in percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dim {
    pub saturation: u8,
    pub light: u8,
}

/// Paused: 20 % colour, 45 % light (notes F).
pub const PAUSED: Dim = Dim {
    saturation: 20,
    light: 45,
};
/// Ended: 30 % colour, 40 % light.
pub const ENDED: Dim = Dim {
    saturation: 30,
    light: 40,
};

/// The stage's texture: one, reused; replaced in place when a newer frame arrived.
#[derive(Clone)]
struct StageTexture {
    tex: egui::TextureHandle,
    /// The number of the frame in it.
    seen: u64,
    /// Its pixels as they came, kept to be refilled once the painter has let go of them.
    image: std::sync::Arc<egui::ColorImage>,
    /// The dimming the texture shows now.
    dim: Option<Dim>,
}

fn key() -> Id {
    Id::unique("chat.stage.picture")
}

/// The live picture of the watched stream in `rect`, fitted on black and dimmed by `dim`;
/// the still stand-in while the slot holds no frame (an earlier stream's picture is never
/// shown). `frames: None` draws the last picture kept (the ended stage), else the stand-in.
/// At most one upload per frame drawn, and none when nothing new came; the decoder wakes
/// the page for each new frame. Returns the picture's size in pixels, once there is one.
pub fn picture(
    ui: &Ui,
    rect: Rect,
    title: &str,
    alpha: f32,
    frames: Option<&zaun_core::service::FrameSlot>,
    dim: Option<Dim>,
) -> Option<[usize; 2]> {
    let painter = ui.painter();
    if frames.is_some_and(|s| !s.has_frame()) {
        still(painter, rect, title, alpha);
        return None;
    }
    // Kept in egui's map as `Option<StageTexture>`. Looked at by a clone (no allocation) on
    // frames without a new picture; taken out when one arrived or the dimming changed, so
    // its pixels are not shared and are refilled in place.
    let seen: Option<u64> = ui.data(|d| {
        d.get_temp::<Option<StageTexture>>(key())
            .flatten()
            .map(|s| s.seen)
    });
    if let Some(slot) = frames.filter(|s| s.has_frame()) {
        // The slot outlives a window: each new window (no texture here yet) wakes itself.
        if seen.is_none() || !slot.has_waker() {
            let ctx = ui.ctx().clone();
            slot.set_waker(Some(std::sync::Arc::new(move || ctx.request_repaint())));
        }
        if let Some((n, f)) = slot.newer_than(seen.unwrap_or(0)) {
            note_frame(ui, n);
            let st = ui.data_mut(|d| d.remove_temp::<Option<StageTexture>>(key()).flatten());
            let st = upload(ui, st, n, &f);
            ui.data_mut(|d| d.insert_temp(key(), Some(st)));
        }
    }
    let applied = ui.data(|d| {
        d.get_temp::<Option<StageTexture>>(key())
            .flatten()
            .map(|s| s.dim)
    });
    if applied.is_some_and(|a| a != dim) {
        let st = ui.data_mut(|d| d.remove_temp::<Option<StageTexture>>(key()).flatten());
        let st = st.map(|s| dimmed(s, dim));
        ui.data_mut(|d| d.insert_temp(key(), st));
    }
    let shown = ui.data(|d| {
        d.get_temp::<Option<StageTexture>>(key())
            .flatten()
            .map(|s| (s.tex.id(), s.tex.size()))
    });
    let Some((tex, size)) = shown else {
        still(painter, rect, title, alpha);
        if dim.is_some() {
            let corner = CornerRadius::same(RADIUS);
            painter.rect_filled(rect, corner, fade(Color32::BLACK, 0.6 * alpha));
        }
        return None;
    };
    let corner = CornerRadius::same(RADIUS);
    painter.rect_filled(rect, corner, fade(Color32::BLACK, alpha));
    let [w, h] = size.map(|v| v.max(1) as f32);
    let scale = (rect.width() / w).min(rect.height() / h);
    let inner = Rect::from_center_size(rect.center(), vec2(w * scale, h * scale));
    egui::Image::from_texture(egui::load::SizedTexture::new(tex, inner.size()))
        .corner_radius(corner)
        .tint(fade(Color32::WHITE, alpha))
        .paint_at(ui, inner);
    Some(size)
}

/// The frames that arrived in the last second: (when, frame number). Eviction: older than
/// one second, so at most one entry per frame of that second.
#[derive(Clone, Default)]
struct Arrivals(std::collections::VecDeque<(f64, u64)>);

fn arrivals_key() -> Id {
    Id::unique("chat.stage.arrivals")
}

fn note_frame(ui: &Ui, n: u64) {
    let now = ui.input(|i| i.time);
    ui.data_mut(|d| {
        let a = d.get_temp_mut_or_default::<Arrivals>(arrivals_key());
        a.0.push_back((now, n));
        while a.0.front().is_some_and(|(t, _)| now - t > 1.0) {
            a.0.pop_front();
        }
    });
}

/// Frames a second received over the last second, from the slot's frame numbers (frames
/// the page did not draw still count); `None` until two have arrived within it.
pub fn received_fps(ui: &Ui) -> Option<f32> {
    let now = ui.input(|i| i.time);
    let a = ui.data(|d| d.get_temp::<Arrivals>(arrivals_key()))?;
    let (t0, n0) = *a.0.iter().find(|(t, _)| now - t <= 1.0)?;
    let (t1, n1) = *a.0.back()?;
    let span = (t1 - t0) as f32;
    (n1 > n0 && span > 0.2).then(|| (n1 - n0) as f32 / span)
}

/// Forget the last picture (a new stream starts from its stand-in).
pub fn forget(ui: &Ui) {
    ui.data_mut(|d| {
        d.remove_temp::<Option<StageTexture>>(key());
        d.remove_temp::<Arrivals>(arrivals_key());
    });
}

/// Put frame `n` into the stage's texture (made on the first frame), refilling its pixel
/// buffer in place unless the painter still holds the last upload.
fn upload(
    ui: &Ui,
    st: Option<StageTexture>,
    n: u64,
    f: &zaun_core::service::VideoFrame,
) -> StageTexture {
    let size = [f.width as usize, f.height as usize];
    let (tex, mut image) = match st {
        Some(s) => (Some(s.tex), s.image),
        None => (None, std::sync::Arc::default()),
    };
    let img = std::sync::Arc::make_mut(&mut image);
    img.size = size;
    img.source_size = vec2(size[0] as f32, size[1] as f32);
    img.pixels.resize(size[0] * size[1], Color32::BLACK);
    for (px, c) in img.pixels.iter_mut().zip(f.rgba.as_chunks::<4>().0) {
        *px = Color32::from_rgb(c[0], c[1], c[2]);
    }
    let data = egui::ImageData::Color(std::sync::Arc::clone(&image));
    let tex = match tex {
        Some(mut t) => {
            t.set(data, egui::TextureOptions::LINEAR);
            t
        }
        None => ui
            .ctx()
            .load_texture("chat.stage.picture", data, egui::TextureOptions::LINEAR),
    };
    StageTexture {
        tex,
        seen: n,
        image,
        dim: None,
    }
}

/// The texture showing the kept pixels with `dim` (one pass when the dimming changes).
fn dimmed(mut st: StageTexture, dim: Option<Dim>) -> StageTexture {
    let data = match dim {
        None => egui::ImageData::Color(std::sync::Arc::clone(&st.image)),
        Some(d) => {
            let (sat, light) = (f32::from(d.saturation) / 100.0, f32::from(d.light) / 100.0);
            let mut img = (*st.image).clone();
            for px in &mut img.pixels {
                let [r, g, b, _] = px.to_array().map(f32::from);
                let lum = 0.299 * r + 0.587 * g + 0.114 * b;
                let f = |c: f32| ((lum + (c - lum) * sat) * light).round().clamp(0.0, 255.0) as u8;
                *px = Color32::from_rgb(f(r), f(g), f(b));
            }
            egui::ImageData::Color(std::sync::Arc::new(img))
        }
    };
    st.tex.set(data, egui::TextureOptions::LINEAR);
    st.dim = dim;
    st
}

/// A still stand-in for the live picture: the game's paper with two hills and a sun in its
/// ink (the prototype's picture, not moving).
pub fn still(painter: &Painter, rect: Rect, title: &str, alpha: f32) {
    let palette::Paper { paper, ink } = palette::paper_for(title);
    let corner = CornerRadius::same(RADIUS);
    painter.rect_filled(rect, corner, fade(Color32::BLACK, alpha));
    painter.rect_filled(rect, corner, fade(paper, alpha));
    let (w, h) = (rect.width(), rect.height());
    // The sun: 13 % of the width, 62 % across and 16 % down.
    let d = w * 0.13;
    painter.circle_filled(
        pos2(
            rect.left() + w * 0.62 + d / 2.0,
            rect.top() + h * 0.16 + d / 2.0,
        ),
        d / 2.0,
        fade(mix(paper, ink, 0.8), alpha),
    );
    hill(painter, rect, 0.40, fade(mix(paper, ink, 0.22), alpha));
    hill(painter, rect, 0.26, fade(mix(paper, ink, 0.5), alpha));
}

/// A hill: the top half of an ellipse twice the stage's width and `share` of its height,
/// standing on its bottom, inside its rounded corners.
fn hill(painter: &Painter, rect: Rect, share: f32, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let (cx, base, rx, ry) = (rect.center().x, rect.bottom(), w, h * share);
    let r = f32::from(RADIUS);
    let mut pts: Vec<Pos2> = Vec::new();
    let steps = 32;
    for i in 0..=steps {
        let x = rect.left() + w * i as f32 / steps as f32;
        let dx = (x - cx) / rx;
        let y = base - ry * (1.0 - dx * dx).max(0.0).sqrt();
        pts.push(pos2(x, y.max(rect.top())));
    }
    // Down the right side and round the bottom corners.
    for i in 0..=8 {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
        pts.push(pos2(rect.right() - r + r * a.cos(), base - r + r * a.sin()));
    }
    for i in 0..=8 {
        let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
        pts.push(pos2(rect.left() + r + r * a.cos(), base - r + r * a.sin()));
    }
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}
