//! Sharing a game (ADR-0036): the capture thread opens the game's window (the portal asks
//! the user), reports "ready", and from then on reads pictures; while the friend watches it
//! encodes them and hands each unit to the stream's session. No picture for two seconds is
//! "paused" (Wayland tells no one which window is in front; a hidden or minimised window
//! sends nothing). Pictures are paced to the chosen rate, encoded on the GPU when it can
//! (`video::encoder`), and a new height or rate takes effect at the next picture.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use easyplay_core::chat::media::{Negotiation, ShareConfig, StreamEvent, StreamMedia, StreamSink};
use easyplay_core::error::{ErrorCode, Result};
use easyplay_core::model::StreamInfo;

use super::media_config;
use crate::clock::now;
use crate::factory::RtcMedia;
use crate::session::video::{self, Role, ShareFlags, VideoMsg, VideoPart, stream_media_sink};
use crate::session::{Inbox, Options};
use crate::video::encoder::{EncodeSpec, EncoderKind, Encoders, kinds};
use crate::video::i420::I420;
use crate::video::pace::{Pacer, RateMeter};
use crate::video::source::{Capture, CaptureError, CaptureRequest, Next};

/// No picture for this long: the window is hidden or minimised.
const PAUSED_AFTER: Duration = Duration::from_secs(2);
/// How long one wait for a picture lasts (the stop flag is checked between waits).
const WAIT: Duration = Duration::from_millis(250);
/// How long stopping a share waits for its capture thread.
const STOP_WAIT: Duration = Duration::from_secs(1);

/// What the handle and the capture thread share.
struct Shared {
    stop: AtomicBool,
    flags: Arc<ShareFlags>,
    /// The stream's session, once the friend's app took the stream.
    session: Mutex<Option<Sender<Inbox>>>,
    /// The chosen height at most and frame rate, and whether they changed.
    quality: Mutex<(Option<u32>, u32)>,
    changed: AtomicBool,
    /// The encoders to try, in order.
    kinds: Vec<Arc<dyn EncoderKind>>,
}

impl Shared {
    fn post(&self, msg: VideoMsg) {
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = session.as_ref() {
            // A session that ended has reported why.
            let _ = tx.send(Inbox::Video(msg));
        }
    }
}

pub(super) fn start(
    m: &RtcMedia,
    config: ShareConfig,
    sink: StreamSink,
) -> Result<Box<dyn StreamMedia>> {
    let shared = Arc::new(Shared {
        stop: AtomicBool::new(false),
        flags: Arc::default(),
        session: Mutex::new(None),
        quality: Mutex::new((config.max_height, config.fps)),
        changed: AtomicBool::new(false),
        kinds: m.encoders().unwrap_or_else(|| kinds(config.hardware)),
    });
    let capture = m.capture();
    let (s, c, k) = (Arc::clone(&shared), config.clone(), Arc::clone(&sink));
    let (alive, ended) = channel::<()>();
    std::thread::Builder::new()
        .name("share-capture".into())
        .spawn(move || {
            let _alive = alive;
            run(&*capture, &c, &s, &k);
        })
        .map_err(|e| easyplay_core::err!(ChtCaptureFailed).with_source(e))?;
    Ok(Box::new(RtcShare {
        shared,
        opts: m.options(),
        config,
        sink,
        ended,
    }))
}

/// The share as the flow holds it. Dropping it stops the capture and the connection.
struct RtcShare {
    shared: Arc<Shared>,
    opts: Options,
    config: ShareConfig,
    sink: StreamSink,
    /// Closes when the capture thread is done (its encoder released).
    ended: std::sync::mpsc::Receiver<()>,
}

impl StreamMedia for RtcShare {
    fn offer(&self, sdp: String) {
        let mut session = self
            .shared
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if session.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let part = VideoPart::new(
            Role::Share {
                flags: Arc::clone(&self.shared.flags),
                title: self.config.title.clone(),
            },
            Arc::clone(&self.sink),
        );
        let config = media_config(
            Negotiation::Answer(sdp),
            self.config.ice.clone(),
            self.config.relay_only,
        );
        let media_sink = stream_media_sink(Arc::clone(&self.sink));
        match video::spawn(
            config,
            media_sink,
            self.opts.clone(),
            (tx.clone(), rx),
            part,
        ) {
            Ok(()) => *session = Some(tx),
            Err(e) => (self.sink)(StreamEvent::Failed {
                code: ErrorCode::ChtStreamFailed,
                detail: e.to_string(),
            }),
        }
    }

    fn answer(&self, _sdp: String) {
        tracing::debug!("stream: an answer for a share (the friend's side offers)");
    }

    fn candidates(&self, candidates: Vec<String>) {
        let session = self
            .shared
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = session.as_ref() {
            let _ = tx.send(Inbox::Candidates(candidates));
        }
    }

    fn watch(&self, _on: bool) {}

    fn mute(&self, _on: bool) {}

    fn set_quality(&self, max_height: Option<u32>, fps: u32) {
        *self
            .shared
            .quality
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = (max_height, fps);
        self.shared.changed.store(true, Ordering::Relaxed);
    }
}

impl Drop for RtcShare {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let session = self
            .shared
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(tx) = session.as_ref() {
            let _ = tx.send(Inbox::Stop);
        }
        drop(session);
        // Let the capture thread release the GPU encoder before going on: a process that
        // exits while NVENC is in use can hang in the driver's exit handlers. A thread still
        // waiting for the portal's dialog is not waited for.
        let _ = self.ended.recv_timeout(STOP_WAIT);
    }
}

/// The capture thread.
fn run(capture: &dyn Capture, config: &ShareConfig, shared: &Shared, sink: &StreamSink) {
    let req = CaptureRequest {
        game: config.game.clone(),
    };
    let mut source = match capture.open(&req) {
        Ok(s) => s,
        Err(e) => {
            let (code, detail) = match e {
                CaptureError::Refused(d) => (ErrorCode::ChtShareRefused, d),
                CaptureError::Failed(d) => (ErrorCode::ChtCaptureFailed, d),
            };
            return sink(StreamEvent::Failed { code, detail });
        }
    };
    if shared.stop.load(Ordering::Relaxed) {
        return;
    }
    sink(StreamEvent::Ready);
    let mut out = Outgoing::new(config, shared, sink);
    let mut pic = I420::default();
    let (mut last, mut paused) = (now(), false);
    while !shared.stop.load(Ordering::Relaxed) {
        out.follow_quality();
        match source.next(&mut pic, out.max_height, WAIT) {
            Next::Frame => {
                last = now();
                if paused {
                    paused = false;
                    note_paused(shared, sink, false);
                }
            }
            Next::Idle => {
                if !paused && last.elapsed() >= PAUSED_AFTER {
                    paused = true;
                    note_paused(shared, sink, true);
                }
                // A window that does not change sends nothing new: a friend who starts
                // watching (or a new picture size) gets the last picture as a keyframe.
                if pic.width == 0 || !shared.flags.keyframe.load(Ordering::Relaxed) {
                    continue;
                }
            }
            Next::Ended => return sink(StreamEvent::Ended),
            Next::Failed(detail) => {
                return sink(StreamEvent::Failed {
                    code: ErrorCode::ChtCaptureFailed,
                    detail,
                });
            }
        }
        if let Err(detail) = out.send(&pic) {
            return sink(StreamEvent::Failed {
                code: ErrorCode::ChtCaptureFailed,
                detail,
            });
        }
    }
}

/// What goes out: the chosen quality, the pacing, the encoder, and what was reported.
struct Outgoing<'a> {
    shared: &'a Shared,
    sink: &'a StreamSink,
    max_height: Option<u32>,
    fps: u32,
    pacer: Pacer,
    meter: RateMeter,
    encoders: Encoders,
    unit: Vec<u8>,
    reported: Option<StreamInfo>,
}

impl<'a> Outgoing<'a> {
    fn new(config: &ShareConfig, shared: &'a Shared, sink: &'a StreamSink) -> Self {
        Self {
            shared,
            sink,
            max_height: config.max_height,
            fps: config.fps,
            pacer: Pacer::new(config.fps),
            meter: RateMeter::new(),
            encoders: Encoders::new(shared.kinds.clone()),
            unit: Vec::new(),
            reported: None,
        }
    }

    /// Take a quality the user chose while sharing: the next picture is a keyframe.
    fn follow_quality(&mut self) {
        if !self.shared.changed.swap(false, Ordering::Relaxed) {
            return;
        }
        let (h, fps) = *self
            .shared
            .quality
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        tracing::info!(?h, fps, "share: the picture changes");
        self.max_height = h;
        self.fps = fps;
        self.pacer.set_fps(fps);
        self.meter = RateMeter::new();
        self.shared.flags.keyframe.store(true, Ordering::Relaxed);
    }

    /// Encode and send `pic` if the friend watches and the pacing lets it through.
    fn send(&mut self, pic: &I420) -> Result<(), String> {
        let flags = &self.shared.flags;
        let key_wanted = flags.keyframe.load(Ordering::Relaxed);
        let at = now();
        if !flags.watched.load(Ordering::Relaxed) || !(self.pacer.take(at) || key_wanted) {
            return Ok(());
        }
        let key = flags.keyframe.swap(false, Ordering::Relaxed);
        let spec = EncodeSpec::new(pic.width, pic.height, self.fps);
        if self.encoders.encode(pic, spec, key, &mut self.unit)? {
            self.shared.post(VideoMsg::Unit {
                unit: std::mem::take(&mut self.unit),
                at,
            });
            let measured = self.meter.sent(at);
            self.report(pic, measured);
        }
        Ok(())
    }

    /// Say what is sent when the encoder, the size or the rate changed.
    fn report(&mut self, pic: &I420, measured: Option<u32>) {
        let Some((encoder, _)) = self.encoders.in_use() else {
            return;
        };
        // Until a rate is measured (after a change too), the chosen one.
        let fps = measured.unwrap_or(if self.meter.fps == 0 {
            self.fps
        } else {
            self.meter.fps
        });
        let info = StreamInfo {
            encoder,
            width: pic.width as u32,
            height: pic.height as u32,
            fps: fps.min(self.fps),
        };
        let same = self.reported.is_some_and(|r| {
            r.encoder == info.encoder
                && (r.width, r.height) == (info.width, info.height)
                && r.fps.abs_diff(info.fps) < 2
        });
        if !same {
            self.reported = Some(info);
            (self.sink)(StreamEvent::Sending(info));
        }
    }
}

fn note_paused(shared: &Shared, sink: &StreamSink, on: bool) {
    sink(StreamEvent::Paused(on));
    shared.post(VideoMsg::Note(
        serde_json::json!({ "paused": on }).to_string(),
    ));
}
