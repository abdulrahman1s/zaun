//! An engine for tests: records requests and lets the test report events as the engine.

use std::sync::{Arc, Mutex};

use super::engine::{ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, Request};
use crate::error::{Error, ErrorCode, Result};

#[derive(Default)]
struct Shared {
    available: bool,
    fail_start: bool,
    starts: Vec<EngineConfig>,
    requests: Vec<Request>,
    sink: Option<EventSink>,
    running: bool,
}

/// A factory whose engines do nothing but record. Clones share their state.
#[derive(Clone)]
pub struct FakeEngine(Arc<Mutex<Shared>>);

impl Default for FakeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeEngine {
    /// An available engine that starts.
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

    /// Make the engine unavailable (as a build without the helper).
    pub fn set_available(&self, available: bool) {
        self.with(|s| s.available = available);
    }

    /// Make starts fail.
    pub fn set_fail_start(&self, fail: bool) {
        self.with(|s| s.fail_start = fail);
    }

    /// Every start's configuration so far.
    pub fn starts(&self) -> Vec<EngineConfig> {
        self.with(|s| s.starts.clone())
    }

    /// Every request so far (and forget them).
    pub fn take_requests(&self) -> Vec<Request> {
        self.with(|s| std::mem::take(&mut s.requests))
    }

    /// Whether an engine runs (started and not dropped).
    pub fn running(&self) -> bool {
        self.with(|s| s.running)
    }

    /// Report `event` as the running engine (no-op when none runs).
    pub fn emit(&self, event: ChatEvent) {
        let sink = self.with(|s| s.sink.clone());
        if let Some(sink) = sink {
            sink(event);
        }
    }
}

impl EngineFactory for FakeEngine {
    fn available(&self) -> bool {
        self.with(|s| s.available)
    }

    fn start(&self, config: EngineConfig, sink: EventSink) -> Result<Box<dyn Engine>> {
        self.with(|s| {
            s.starts.push(config);
            if !s.available {
                return Err(Error::new(ErrorCode::ChtUnavailable));
            }
            if s.fail_start {
                return Err(Error::new(ErrorCode::ChtUnavailable));
            }
            s.sink = Some(sink);
            s.running = true;
            Ok(Box::new(FakeRunning(self.clone())) as Box<dyn Engine>)
        })
    }
}

struct FakeRunning(FakeEngine);

impl Engine for FakeRunning {
    fn request(&self, request: Request) {
        self.0.with(|s| s.requests.push(request));
    }
}

impl Drop for FakeRunning {
    fn drop(&mut self) {
        self.0.with(|s| {
            s.running = false;
            s.sink = None;
        });
    }
}
