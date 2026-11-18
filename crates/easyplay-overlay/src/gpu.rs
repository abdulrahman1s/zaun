//! The GPU side: a wgpu device presenting egui's triangles into the layer surface with
//! premultiplied alpha, so everything egui leaves empty shows the game underneath.
use egui::epaint::{ClippedPrimitive, textures::TexturesDelta};
use wayland_client::Connection;
use wayland_client::protocol::wl_surface::WlSurface;

use crate::surface::{self, Display};

pub struct Gpu {
    renderer: egui_wgpu::Renderer,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    /// A device and surface for `wl` at `size` pixels. Vulkan first, then GL (EGL).
    pub fn new(conn: &Connection, wl: &WlSurface, size: (u32, u32)) -> Result<Self, String> {
        let display = || Display::of(conn).ok_or("not connected through libwayland");
        let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display()?));
        desc.backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
        let instance = wgpu::Instance::new(desc);
        let surface = surface::create(&instance, &display()?, wl)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .map_err(|e| e.to_string())?;
        tracing::info!(adapter = ?adapter.get_info(), "overlay: GPU");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let caps = surface.get_capabilities(&adapter);
        let format =
            egui_wgpu::preferred_framebuffer_format(&caps.formats).map_err(|e| e.to_string())?;
        // Opaque would hide the game: without a transparent mode there is no overlay.
        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m))
        .ok_or_else(|| format!("no transparent presentation ({:?})", caps.alpha_modes))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::default(),
            width: size.0.max(1),
            height: size.1.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);
        let renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        Ok(Self {
            renderer,
            surface,
            device,
            queue,
            config,
        })
    }

    /// The surface's size in pixels changed.
    pub fn resize(&mut self, size: (u32, u32)) {
        let size = (size.0.max(1), size.1.max(1));
        if (self.config.width, self.config.height) != size {
            (self.config.width, self.config.height) = size;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Draw one frame and present it (this commits the Wayland surface). Texture changes are
    /// applied even when there is no frame to draw into, or they would be lost.
    pub fn render(&mut self, prims: &[ClippedPrimitive], mut delta: TexturesDelta, ppp: f32) {
        for (id, images) in &delta.set {
            for image in images {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, image);
            }
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => {
                tracing::debug!(?other, "overlay: no frame; reconfiguring");
                self.surface.configure(&self.device, &self.config);
                self.free(&mut delta);
                return;
            }
        };
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: ppp,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let extra =
            self.renderer
                .update_buffers(&self.device, &self.queue, &mut encoder, prims, &screen);
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), prims, &screen);
        }
        self.queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        self.queue.present(frame);
        self.free(&mut delta);
    }

    /// Free what egui is done with (after the submit that may still use it).
    fn free(&mut self, delta: &mut TexturesDelta) {
        for id in &delta.free {
            self.renderer.free_texture(id);
        }
        delta.clear();
    }
}
