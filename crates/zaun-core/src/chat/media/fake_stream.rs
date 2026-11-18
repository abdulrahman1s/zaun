//! Streams for tests and the preview (ADR-0036): records what the share flow asks and lets
//! the test report events as the media. Watching publishes the test picture once.
use std::sync::{Arc, Mutex, PoisonError};

use super::frame::FrameSlot;
use super::pattern::test_pattern;
use super::stream::{
    ShareConfig, StreamEvent, StreamFactory, StreamMedia, StreamSink, WatchConfig,
};
use crate::error::{Error, ErrorCode, Result};

/// One stream the fake started.
struct Running {
    id: u64,
    sharing: bool,
    sink: StreamSink,
}

#[derive(Default)]
struct Shared {
    fail_start: Option<ErrorCode>,
    /// Windows cannot be captured (a build without capture).
    no_capture: bool,
    next: u64,
    shares: Vec<ShareConfig>,
    watches: u32,
    running: Vec<Running>,
    offers: Vec<String>,
    answers: Vec<String>,
    /// The last `watch` and `mute` asked of a watched stream.
    watching: bool,
    muted: bool,
    frames: Option<FrameSlot>,
    /// Each `set_quality` asked of a share.
    qualities: Vec<(Option<u32>, u32)>,
}

/// A stream factory whose media does nothing but record. Clones share their state.
#[derive(Clone, Default)]
pub struct FakeStreams(Arc<Mutex<Shared>>);

impl FakeStreams {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Shared) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Make starts fail with `code` (`None`: they start).
    pub fn set_fail_start(&self, code: Option<ErrorCode>) {
        self.with(|s| s.fail_start = code);
    }

    /// Make windows impossible to capture (`can_share` false).
    pub fn set_no_capture(&self, on: bool) {
        self.with(|s| s.no_capture = on);
    }

    /// Every share's configuration so far.
    pub fn shares(&self) -> Vec<ShareConfig> {
        self.with(|s| s.shares.clone())
    }

    /// How many friends' streams were taken so far.
    pub fn watches(&self) -> u32 {
        self.with(|s| s.watches)
    }

    /// Streams running now: (shared, watched).
    pub fn running(&self) -> (usize, usize) {
        self.with(|s| {
            let shared = s.running.iter().filter(|r| r.sharing).count();
            (shared, s.running.len() - shared)
        })
    }

    /// Offers handed to a share so far.
    pub fn offers(&self) -> Vec<String> {
        self.with(|s| s.offers.clone())
    }

    /// The qualities asked of shares so far: (height at most, frames a second).
    pub fn qualities(&self) -> Vec<(Option<u32>, u32)> {
        self.with(|s| s.qualities.clone())
    }

    /// Answers handed to a watched stream so far.
    pub fn answers(&self) -> Vec<String> {
        self.with(|s| s.answers.clone())
    }

    /// The last (watch, mute) asked of a watched stream.
    pub fn controls(&self) -> (bool, bool) {
        self.with(|s| (s.watching, s.muted))
    }

    /// Report `event` as the running share (no-op when none runs).
    pub fn emit_share(&self, event: StreamEvent) {
        self.emit_where(true, event);
    }

    /// Report `event` as the newest watched stream (no-op when none runs).
    pub fn emit_watch(&self, event: StreamEvent) {
        self.emit_where(false, event);
    }

    fn emit_where(&self, sharing: bool, event: StreamEvent) {
        let sink = self.with(|s| {
            s.running
                .iter()
                .rev()
                .find(|r| r.sharing == sharing)
                .map(|r| Arc::clone(&r.sink))
        });
        if let Some(sink) = sink {
            sink(event);
        }
    }

    fn start(&self, sharing: bool, sink: StreamSink) -> Result<Box<dyn StreamMedia>> {
        self.with(|s| {
            if let Some(code) = s.fail_start {
                return Err(Error::new(code));
            }
            s.next += 1;
            let id = s.next;
            s.running.push(Running { id, sharing, sink });
            Ok(Box::new(FakeStream {
                streams: self.clone(),
                id,
            }) as Box<dyn StreamMedia>)
        })
    }
}

impl StreamFactory for FakeStreams {
    fn share(&self, config: ShareConfig, sink: StreamSink) -> Result<Box<dyn StreamMedia>> {
        self.with(|s| s.shares.push(config));
        self.start(true, sink)
    }

    fn watch(&self, config: WatchConfig, sink: StreamSink) -> Result<Box<dyn StreamMedia>> {
        self.with(|s| {
            s.watches += 1;
            s.frames = Some(config.frames);
        });
        self.start(false, sink)
    }

    fn can_share(&self) -> bool {
        self.with(|s| !s.no_capture)
    }
}

struct FakeStream {
    streams: FakeStreams,
    id: u64,
}

impl StreamMedia for FakeStream {
    fn offer(&self, sdp: String) {
        self.streams.with(|s| s.offers.push(sdp));
    }

    fn answer(&self, sdp: String) {
        self.streams.with(|s| s.answers.push(sdp));
    }

    fn candidates(&self, _candidates: Vec<String>) {}

    fn watch(&self, on: bool) {
        let frames = self.streams.with(|s| {
            s.watching = on;
            s.frames.clone()
        });
        if let Some(f) = frames.filter(|_| on) {
            f.publish(test_pattern(320, 180, 0));
        }
    }

    fn mute(&self, on: bool) {
        self.streams.with(|s| s.muted = on);
    }

    fn set_quality(&self, max_height: Option<u32>, fps: u32) {
        self.streams.with(|s| s.qualities.push((max_height, fps)));
    }
}

impl Drop for FakeStream {
    fn drop(&mut self) {
        self.streams.with(|s| s.running.retain(|r| r.id != self.id));
    }
}

/// Call media and streams together, for tests of the whole Chat flow: calls go to `calls`,
/// streams to `streams`.
#[derive(Clone, Default)]
pub struct FakeAv {
    pub calls: super::fake::FakeMedia,
    pub streams: FakeStreams,
}

impl super::seam::MediaFactory for FakeAv {
    fn start(
        &self,
        config: super::seam::MediaConfig,
        sink: super::seam::MediaSink,
    ) -> Result<Box<dyn super::seam::CallMedia>> {
        self.calls.start(config, sink)
    }

    fn wait(&self, wait: std::time::Duration) -> std::time::Duration {
        self.calls.wait(wait)
    }

    fn streams(&self) -> Option<Arc<dyn StreamFactory>> {
        Some(Arc::new(self.streams.clone()))
    }
}
