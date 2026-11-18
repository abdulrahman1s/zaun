//! The call media factory and one running call (ADR-0035).
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Sender, channel};

use rustls::pki_types::CertificateDer;
use zaun_core::chat::media::{CallMedia, MediaConfig, MediaFactory, MediaSink};
use zaun_core::err;
use zaun_core::error::Result;

use crate::audio::backend::AudioBackend;
use crate::net::stream::tls_config;
use crate::session::{self, Inbox, Options};
use crate::video::source::Capture;

/// Starts calls' media: WebRTC through str0m, sound through `audio`. Also streams games
/// (ADR-0036, `crate::stream`), their pictures from `capture`.
#[derive(Clone)]
pub struct RtcMedia {
    opts: Options,
    capture: Arc<dyn Capture>,
    /// The share encoders to try (`None`: by `ShareConfig::hardware`); tests choose.
    encoders: Option<Vec<Arc<dyn crate::video::encoder::EncoderKind>>>,
}

impl RtcMedia {
    /// Calls with the system's sound (PulseAudio or PipeWire).
    pub fn new(audio: Arc<dyn AudioBackend>) -> Self {
        let tls = tls_config(&[])
            .inspect_err(|e| tracing::warn!(error = %e, "call: TLS for relays not available"))
            .ok();
        Self {
            opts: Options {
                audio,
                host_ips: None,
                tls,
            },
            capture: crate::capture::system(),
            encoders: None,
        }
    }

    /// Encode shares with these encoders, in this order (tests: only the GPU's, or only
    /// software).
    #[must_use]
    pub fn with_encoders(
        mut self,
        kinds: Vec<Arc<dyn crate::video::encoder::EncoderKind>>,
    ) -> Self {
        self.encoders = Some(kinds);
        self
    }

    pub(crate) fn encoders(&self) -> Option<Vec<Arc<dyn crate::video::encoder::EncoderKind>>> {
        self.encoders.clone()
    }

    /// Take shared games' pictures from `capture` (tests: the test picture).
    #[must_use]
    pub fn with_capture(mut self, capture: Arc<dyn Capture>) -> Self {
        self.capture = capture;
        self
    }

    pub(crate) fn options(&self) -> Options {
        self.opts.clone()
    }

    pub(crate) fn capture(&self) -> Arc<dyn Capture> {
        Arc::clone(&self.capture)
    }

    /// Use these addresses for direct candidates instead of the default route's (tests use
    /// loopback).
    #[must_use]
    pub fn with_host_ips(mut self, ips: Vec<IpAddr>) -> Self {
        self.opts.host_ips = Some(ips);
        self
    }

    /// Also trust these certificates for `turns:` relays (tests' own relay).
    pub fn with_extra_roots(mut self, roots: &[CertificateDer<'static>]) -> std::io::Result<Self> {
        self.opts.tls = Some(tls_config(roots)?);
        Ok(self)
    }
}

impl MediaFactory for RtcMedia {
    fn start(&self, config: MediaConfig, sink: MediaSink) -> Result<Box<dyn CallMedia>> {
        let (tx, rx) = channel();
        let muted = Arc::new(AtomicBool::new(false));
        session::spawn(config, sink, self.opts.clone(), tx.clone(), rx, muted)
            .map_err(|e| err!(ChtMediaFailed).with_source(e))?;
        Ok(Box::new(RtcCall { tx }))
    }

    fn streams(&self) -> Option<Arc<dyn zaun_core::chat::media::StreamFactory>> {
        Some(Arc::new(self.clone()))
    }

    fn devices(&self) -> Result<zaun_core::model::AudioDevices> {
        self.opts
            .audio
            .devices()
            .map_err(|e| err!(ChtMediaFailed).with_source(e))
    }

    fn mic_test(
        &self,
        audio: zaun_core::chat::media::AudioSettings,
        sink: zaun_core::chat::media::MicTestSink,
    ) -> Result<Box<dyn zaun_core::chat::media::MicTest>> {
        let running = crate::audio::mic_test::start(&self.opts.audio, audio, sink)
            .map_err(|e| err!(ChtMediaFailed).with_source(e))?;
        Ok(Box::new(running))
    }
}

/// A running call: requests go to its session thread; dropping it hangs up.
struct RtcCall {
    tx: Sender<Inbox>,
}

impl RtcCall {
    fn send(&self, msg: Inbox) {
        // The session may have ended already (it reported why).
        let _ = self.tx.send(msg);
    }
}

impl CallMedia for RtcCall {
    fn answer(&self, sdp: String) {
        self.send(Inbox::Answer(sdp));
    }

    fn candidates(&self, candidates: Vec<String>) {
        self.send(Inbox::Candidates(candidates));
    }

    fn set_muted(&self, on: bool) {
        self.send(Inbox::Muted(on));
    }

    fn set_deafened(&self, on: bool) {
        self.send(Inbox::Deafened(on));
    }
}

impl Drop for RtcCall {
    fn drop(&mut self) {
        self.send(Inbox::Stop);
    }
}
