//! The overlay's state: the Wayland objects, the GPU, egui and the model, on one thread.
use easyplay_core::clock::{Clock, SystemClock};
use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::registry::RegistryState;
use smithay_client_toolkit::seat::SeatState;
use smithay_client_toolkit::shell::wlr_layer::{LayerShell, LayerSurface};
use wayland_client::globals::GlobalList;
use wayland_client::protocol::{wl_keyboard::WlKeyboard, wl_pointer::WlPointer};
use wayland_client::{Connection, QueueHandle};

use crate::gpu::Gpu;
use crate::model::Model;

pub struct App {
    // Declared first, so dropped first: the wgpu surface points into the layer surface and
    // the connection (`surface::create`).
    pub gpu: Option<Gpu>,
    pub conn: Connection,
    pub qh: QueueHandle<App>,
    pub registry: RegistryState,
    pub seats: SeatState,
    pub outputs: OutputState,
    pub compositor: CompositorState,
    pub layer: LayerSurface,
    pub keyboard: Option<WlKeyboard>,
    pub pointer: Option<WlPointer>,
    pub handle: LoopHandle<'static, App>,

    pub egui: egui::Context,
    /// Input since the last frame.
    pub events: Vec<egui::Event>,
    pub modifiers: egui::Modifiers,
    pub clock: SystemClock,

    pub model: Model,
    /// Logical size from the last configure; `None` until the first.
    pub size: Option<(u32, u32)>,
    pub scale: i32,
    /// Something changed since the last frame.
    pub dirty: bool,
    /// A frame callback is outstanding: draw when it arrives, not before.
    pub waiting_frame: bool,
    /// When egui wants to be drawn again (seconds of `now()`).
    pub repaint_at: Option<f64>,
    /// Frames drawn (the idle check counts them).
    pub frames: u64,
    pub ready_sent: bool,
    pub exit: Option<u8>,
}

impl App {
    pub fn new(
        conn: Connection,
        globals: &GlobalList,
        qh: QueueHandle<App>,
        compositor: CompositorState,
        shell: &LayerShell,
        handle: LoopHandle<'static, App>,
    ) -> Self {
        let layer = crate::layer::create(&compositor, shell, &qh);
        let egui = egui::Context::default();
        crate::style::fonts(&egui);
        crate::style::apply(&egui, &crate::look::Look::new("tide", false));
        Self {
            gpu: None,
            registry: RegistryState::new(globals),
            seats: SeatState::new(globals, &qh),
            outputs: OutputState::new(globals, &qh),
            conn,
            qh,
            compositor,
            layer,
            keyboard: None,
            pointer: None,
            handle,
            egui,
            events: Vec::new(),
            modifiers: egui::Modifiers::default(),
            clock: SystemClock::new(),
            model: Model::new(jiff::Zoned::now().offset().seconds()),
            size: None,
            scale: 1,
            dirty: true,
            waiting_frame: false,
            repaint_at: None,
            frames: 0,
            ready_sent: false,
            exit: None,
        }
    }

    /// Seconds since start (egui's time).
    pub fn now(&self) -> f64 {
        self.clock.mono().as_duration().as_secs_f64()
    }

    pub fn pixels(&self) -> (u32, u32) {
        let (w, h) = self.size.unwrap_or((1, 1));
        let s = self.scale.unsigned_abs().max(1);
        (w * s, h * s)
    }
}
