//! Call media for tests: records what the call flow asks and lets the test report events as
//! the media.
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::seam::{
    AudioSettings, CallMedia, MediaConfig, MediaEvent, MediaFactory, MediaSink, MicTest,
    MicTestEvent, MicTestSink,
};
use crate::error::{ErrorCode, Result};
use crate::model::AudioDevices;

#[derive(Default)]
struct Shared {
    fail_start: Option<ErrorCode>,
    short_waits: bool,
    starts: Vec<MediaConfig>,
    sink: Option<MediaSink>,
    running: bool,
    answers: Vec<String>,
    candidates: Vec<String>,
    muted: bool,
    deafened: bool,
    waits: Vec<Duration>,
    devices: AudioDevices,
    mic_tests: Vec<AudioSettings>,
    mic_sink: Option<MicTestSink>,
}

/// A factory whose media does nothing but record. Clones share their state.
#[derive(Clone, Default)]
pub struct FakeMedia(Arc<Mutex<Shared>>);

impl FakeMedia {
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

    /// Wait a thousandth of each call timeout (20 s → 20 ms), for tests of the timeouts.
    pub fn set_short_waits(&self, on: bool) {
        self.with(|s| s.short_waits = on);
    }

    /// Every start's configuration so far.
    pub fn starts(&self) -> Vec<MediaConfig> {
        self.with(|s| s.starts.clone())
    }

    /// Whether a call's media runs (started and not dropped).
    pub fn running(&self) -> bool {
        self.with(|s| s.running)
    }

    /// The answers handed to the media so far.
    pub fn answers(&self) -> Vec<String> {
        self.with(|s| s.answers.clone())
    }

    /// The friend's candidates handed to the media so far.
    pub fn candidates(&self) -> Vec<String> {
        self.with(|s| s.candidates.clone())
    }

    /// The microphone and the speakers as last set: (muted, deafened).
    pub fn controls(&self) -> (bool, bool) {
        self.with(|s| (s.muted, s.deafened))
    }

    /// The timeouts asked for so far (the real ones).
    pub fn waits(&self) -> Vec<Duration> {
        self.with(|s| s.waits.clone())
    }

    /// What [`MediaFactory::devices`] lists.
    pub fn set_devices(&self, devices: AudioDevices) {
        self.with(|s| s.devices = devices);
    }

    /// Every microphone test's settings so far.
    pub fn mic_tests(&self) -> Vec<AudioSettings> {
        self.with(|s| s.mic_tests.clone())
    }

    /// Whether a microphone test runs (started and not dropped).
    pub fn mic_test_running(&self) -> bool {
        self.with(|s| s.mic_sink.is_some())
    }

    /// Report `event` as the running microphone test (no-op when none runs).
    pub fn emit_mic(&self, event: MicTestEvent) {
        let sink = self.with(|s| s.mic_sink.clone());
        if let Some(sink) = sink {
            sink(event);
        }
    }

    /// Report `event` as the running media (no-op when none runs).
    pub fn emit(&self, event: MediaEvent) {
        let sink = self.with(|s| s.sink.clone());
        if let Some(sink) = sink {
            sink(event);
        }
    }
}

impl MediaFactory for FakeMedia {
    fn start(&self, config: MediaConfig, sink: MediaSink) -> Result<Box<dyn CallMedia>> {
        self.with(|s| {
            s.starts.push(config);
            if let Some(code) = s.fail_start {
                return Err(crate::error::Error::new(code));
            }
            s.sink = Some(sink);
            s.running = true;
            s.muted = false;
            s.deafened = false;
            Ok(Box::new(FakeCall(self.clone())) as Box<dyn CallMedia>)
        })
    }

    fn devices(&self) -> Result<AudioDevices> {
        Ok(self.with(|s| s.devices.clone()))
    }

    fn mic_test(&self, audio: AudioSettings, sink: MicTestSink) -> Result<Box<dyn MicTest>> {
        self.with(|s| {
            s.mic_tests.push(audio);
            if let Some(code) = s.fail_start {
                return Err(crate::error::Error::new(code));
            }
            s.mic_sink = Some(sink);
            Ok(Box::new(FakeMicTest(self.clone())) as Box<dyn MicTest>)
        })
    }

    fn wait(&self, wait: Duration) -> Duration {
        self.with(|s| {
            s.waits.push(wait);
            if s.short_waits { wait / 1000 } else { wait }
        })
    }
}

struct FakeMicTest(FakeMedia);

impl MicTest for FakeMicTest {}

impl Drop for FakeMicTest {
    fn drop(&mut self) {
        self.0.with(|s| s.mic_sink = None);
    }
}

struct FakeCall(FakeMedia);

impl CallMedia for FakeCall {
    fn answer(&self, sdp: String) {
        self.0.with(|s| s.answers.push(sdp));
    }

    fn candidates(&self, candidates: Vec<String>) {
        self.0.with(|s| s.candidates.extend(candidates));
    }

    fn set_muted(&self, on: bool) {
        self.0.with(|s| s.muted = on);
    }

    fn set_deafened(&self, on: bool) {
        self.0.with(|s| s.deafened = on);
    }
}

impl Drop for FakeCall {
    fn drop(&mut self) {
        self.0.with(|s| {
            s.running = false;
            s.sink = None;
        });
    }
}
