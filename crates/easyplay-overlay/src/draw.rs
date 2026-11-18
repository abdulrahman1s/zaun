//! Drawing on demand: a frame only when input, a message or an egui timer asks for one, and
//! never more than one per compositor frame callback. Idle, the overlay draws nothing.
use std::time::Duration;

use easyplay_core::chat::overlay::protocol::{EXIT_NO_GPU, FromOverlay};
use smithay_client_toolkit::shell::WaylandSurface;

use crate::app::App;
use crate::gpu::Gpu;

impl App {
    /// How long the loop may sleep: until egui's next timer, or not at all when a frame is
    /// due now.
    pub fn timeout(&self) -> Option<Duration> {
        if self.size.is_none() || self.waiting_frame {
            return None;
        }
        if self.dirty {
            return Some(Duration::ZERO);
        }
        self.repaint_at
            .map(|t| Duration::from_secs_f64((t - self.now()).max(0.0)))
    }

    /// Draw if something asks for it and the compositor is ready for a frame.
    pub fn maybe_draw(&mut self) {
        if self.size.is_none() || self.exit.is_some() {
            return;
        }
        if self.gpu.is_none() {
            match Gpu::new(&self.conn, self.layer.wl_surface(), self.pixels()) {
                Ok(g) => self.gpu = Some(g),
                Err(e) => {
                    tracing::error!(error = %e, "overlay: cannot draw");
                    self.exit = u8::try_from(EXIT_NO_GPU).ok();
                    return;
                }
            }
        }
        if !self.ready_sent {
            self.ready_sent = true;
            crate::pipe::say(&FromOverlay::Ready);
        }
        let due = self.repaint_at.is_some_and(|t| self.now() >= t);
        if (self.dirty || due) && !self.waiting_frame {
            self.draw();
        }
    }

    fn draw(&mut self) {
        self.dirty = false;
        self.repaint_at = None;
        let now = self.now();
        let (w, h) = self.size.unwrap_or((1, 1));
        #[allow(clippy::cast_precision_loss)]
        let (ppp, size) = (self.scale.max(1) as f32, egui::vec2(w as f32, h as f32));
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(now),
            events: std::mem::take(&mut self.events),
            focused: self.model.open.is_on(),
            ..egui::RawInput::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        let model = &mut self.model;
        let out = self
            .egui
            .run_pass(raw, |ctx| crate::frame::draw(ctx, model, now));
        for m in std::mem::take(&mut self.model.said) {
            if m == FromOverlay::Closed {
                self.set_open(false);
            }
            crate::pipe::say(&m);
        }
        let prims = self.egui.tessellate(out.shapes, out.pixels_per_point);
        let surface = self.layer.wl_surface();
        surface.frame(&self.qh, surface.clone());
        self.waiting_frame = true;
        if let Some(gpu) = &mut self.gpu {
            gpu.render(&prims, out.textures_delta, out.pixels_per_point);
        }
        self.frames += 1;
        let delay = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::MAX, |v| v.repaint_delay);
        if delay.is_zero() {
            self.dirty = true;
        } else if delay < Duration::from_secs(3600) {
            self.repaint_at = Some(now + delay.as_secs_f64());
        }
    }
}
