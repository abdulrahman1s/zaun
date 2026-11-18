//! An overlay for tests: records what the service sends and lets the test speak as the
//! overlay.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::protocol::{FromOverlay, ToOverlay};
use super::seam::{Overlay, OverlayEvent, OverlayFactory, OverlaySink};
use crate::error::Result;

#[derive(Default)]
struct Shared {
    available: bool,
    starts: u32,
    sent: Vec<ToOverlay>,
    sink: Option<OverlaySink>,
    running: bool,
    restart_waits: Vec<Duration>,
    layer_shell: Option<bool>,
    probes: u32,
}

/// A factory whose overlays only record. Clones share their state.
#[derive(Clone)]
pub struct FakeOverlay(Arc<Mutex<Shared>>);

impl Default for FakeOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeOverlay {
    /// An available overlay.
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Shared {
            available: true,
            ..Shared::default()
        })))
    }

    fn with<R>(&self, f: impl FnOnce(&mut Shared) -> R) -> R {
        f(&mut self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    pub fn set_available(&self, available: bool) {
        self.with(|s| s.available = available);
    }

    /// What the desktop reports about its layer shell (`None` = cannot tell, the default).
    pub fn set_layer_shell(&self, has: Option<bool>) {
        self.with(|s| s.layer_shell = has);
    }

    /// How many times the desktop was asked.
    pub fn probes(&self) -> u32 {
        self.with(|s| s.probes)
    }

    /// How many overlays were started so far.
    pub fn starts(&self) -> u32 {
        self.with(|s| s.starts)
    }

    /// Whether one runs (started, not dropped, not exited).
    pub fn running(&self) -> bool {
        self.with(|s| s.running)
    }

    /// Everything sent so far (and forget it).
    pub fn take_sent(&self) -> Vec<ToOverlay> {
        self.with(|s| std::mem::take(&mut s.sent))
    }

    /// The back-offs asked for before each restart (the fake waits a hundredth).
    pub fn restart_waits(&self) -> Vec<Duration> {
        self.with(|s| s.restart_waits.clone())
    }

    /// The running overlay's event sink (to speak as it after it was replaced).
    pub fn sink(&self) -> Option<OverlaySink> {
        self.with(|s| s.sink.clone())
    }

    /// Speak as the running overlay (no-op when none runs).
    pub fn say(&self, m: FromOverlay) {
        self.emit(OverlayEvent::Said(m));
    }

    /// Report `e` as the running overlay; `Exited` also marks it gone.
    pub fn emit(&self, e: OverlayEvent) {
        let sink = self.with(|s| {
            if matches!(e, OverlayEvent::Exited { .. }) {
                s.running = false;
                s.sink.take()
            } else {
                s.sink.clone()
            }
        });
        if let Some(sink) = sink {
            sink(e);
        }
    }
}

impl OverlayFactory for FakeOverlay {
    fn available(&self) -> bool {
        self.with(|s| s.available)
    }

    fn start(&self, sink: OverlaySink) -> Result<Box<dyn Overlay>> {
        self.with(|s| {
            ensure!(s.available, ChtUnavailable);
            s.starts += 1;
            s.sink = Some(sink);
            s.running = true;
            Ok(Box::new(Running(self.clone())) as Box<dyn Overlay>)
        })
    }

    fn has_layer_shell(&self) -> Option<bool> {
        self.with(|s| {
            s.probes += 1;
            s.layer_shell
        })
    }

    fn restart_wait(&self, wait: Duration) -> Duration {
        self.with(|s| s.restart_waits.push(wait));
        wait / 100
    }
}

struct Running(FakeOverlay);

impl Overlay for Running {
    fn send(&self, msg: &ToOverlay) {
        self.0.with(|s| s.sent.push(msg.clone()));
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.with(|s| {
            s.running = false;
            s.sink = None;
        });
    }
}
