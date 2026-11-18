//! A stream's peer connection (ADR-0036): the same session thread as a call, with one H.264
//! video track and a data channel instead of the microphone and speakers. The sharer sends
//! the picture only while the friend watches; small JSON notes on the channel carry the
//! game's title, "paused" and "watch".
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::time::Instant;

use serde_json::Value;
use str0m::channel::{ChannelData, ChannelId};
use str0m::format::Codec;
use str0m::media::{Frequency, KeyframeRequestKind, MediaData, MediaTime, Mid};
use zaun_core::chat::media::{MediaConfig, MediaEvent, MediaSink, StreamEvent, StreamSink};

use super::inbox::Inbox;
use super::run::{Fail, Options, Session};
use crate::clock::now;

/// The data channel's name.
pub(crate) const CHANNEL: &str = "zaun-stream";
/// Notes waiting for the channel to open (more are dropped: only the latest state counts).
const OUTBOX: usize = 8;

/// What a stream's other threads and handle ask of its session.
pub(crate) enum VideoMsg {
    /// An encoded picture (H.264 access unit) taken at `at`.
    Unit { unit: Vec<u8>, at: Instant },
    /// A note for the friend's side (JSON).
    Note(String),
    /// Ask the friend's side for a keyframe.
    Keyframe,
}

/// The sharing side's flags, shared with its capture thread.
#[derive(Default)]
pub(crate) struct ShareFlags {
    /// The friend watches: encode and send.
    pub watched: AtomicBool,
    /// The next picture is a keyframe.
    pub keyframe: AtomicBool,
}

pub(crate) enum Role {
    Share {
        flags: Arc<ShareFlags>,
        /// The game's title, told to the friend when the channel opens.
        title: String,
    },
    Watch {
        /// Receive the picture (the user watches).
        watching: Arc<AtomicBool>,
        /// To the decoder thread (bounded: a full queue drops the unit).
        decoder: SyncSender<Arc<[u8]>>,
    },
}

/// What a session keeps for a stream.
pub(crate) struct VideoPart {
    pub role: Role,
    pub sink: StreamSink,
    pub mid: Option<Mid>,
    channel: Option<ChannelId>,
    outbox: Vec<String>,
    started: Instant,
}

impl VideoPart {
    pub fn new(role: Role, sink: StreamSink) -> Self {
        Self {
            role,
            sink,
            mid: None,
            channel: None,
            outbox: Vec::new(),
            started: now(),
        }
    }
}

/// The session's own events, for a stream: what the call flow would hear becomes stream
/// events; levels mean nothing here.
pub(crate) fn stream_media_sink(sink: StreamSink) -> MediaSink {
    Arc::new(move |e| {
        let event = match e {
            MediaEvent::Description(sdp) => StreamEvent::Description(sdp),
            MediaEvent::Connected => StreamEvent::Connected,
            MediaEvent::Interrupted => StreamEvent::Interrupted,
            MediaEvent::Levels { .. } | MediaEvent::Quality { .. } => return,
            MediaEvent::Failed { code, detail } => {
                tracing::debug!(code = code.as_str(), "stream: the connection failed");
                StreamEvent::Failed {
                    code: zaun_core::error::ErrorCode::ChtStreamFailed,
                    detail,
                }
            }
        };
        sink(event);
    })
}

/// Start a stream's session thread.
pub(crate) fn spawn(
    config: MediaConfig,
    sink: MediaSink,
    opts: Options,
    (tx, rx): (Sender<Inbox>, Receiver<Inbox>),
    part: VideoPart,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("stream-media".into())
        .spawn(move || {
            let muted = Arc::new(AtomicBool::new(true));
            let stop = Arc::new(AtomicBool::new(false));
            let failed =
                match Session::open(&config, &opts, Arc::clone(&sink), tx, rx, muted, &stop) {
                    Ok(mut s) => {
                        s.video = Some(part);
                        let r = s.run(&config, &opts);
                        s.relays_close();
                        r.err()
                    }
                    Err(f) => Some(f),
                };
            stop.store(true, Ordering::Relaxed);
            if let Some(Fail::Error { code, detail }) = failed {
                sink(MediaEvent::Failed { code, detail });
            }
        })?;
    Ok(())
}

impl Session {
    /// A media line was added; video goes to the stream.
    pub(super) fn video_added(&mut self, mid: Mid) {
        if let Some(v) = &mut self.video {
            v.mid = Some(mid);
        }
    }

    /// Whether `mid` is the stream's video.
    pub(super) fn is_video(&self, mid: Mid) -> bool {
        self.video.as_ref().is_some_and(|v| v.mid == Some(mid))
    }

    /// A unit of the friend's picture: to the decoder while the user watches.
    pub(super) fn video_data(&mut self, d: &MediaData) {
        let Some(Role::Watch { watching, decoder }) = self.video.as_ref().map(|v| &v.role) else {
            return;
        };
        if !watching.load(Ordering::Relaxed) {
            return;
        }
        if decoder.try_send(Arc::clone(&d.data)).is_err() {
            tracing::trace!("stream: the decoder is behind; a unit was dropped");
        }
    }

    pub(super) fn video_msg(&mut self, msg: VideoMsg) {
        match msg {
            VideoMsg::Unit { unit, at } => self.send_unit(unit, at),
            VideoMsg::Note(text) => self.note(text),
            VideoMsg::Keyframe => self.ask_keyframe(),
        }
    }

    fn send_unit(&mut self, unit: Vec<u8>, at: Instant) {
        let (Some(mid), true) = (self.video.as_ref().and_then(|v| v.mid), self.connected) else {
            return;
        };
        let started = self.video.as_ref().map_or(at, |v| v.started);
        let Some(writer) = self.rtc.writer(mid) else {
            return;
        };
        let h264 = |mode1: bool| {
            writer
                .payload_params()
                .find(|p| {
                    let s = p.spec();
                    s.codec == Codec::H264 && (!mode1 || s.format.packetization_mode == Some(1))
                })
                .map(|p| p.pt())
        };
        let Some(pt) = h264(true).or_else(|| h264(false)) else {
            return;
        };
        let ticks = at.saturating_duration_since(started).as_micros() as u64 * 9 / 100;
        let time = MediaTime::new(ticks, Frequency::NINETY_KHZ);
        if let Err(e) = writer.write(pt, at, time, unit) {
            tracing::debug!(error = %e, "stream: a unit was not sent");
        }
    }

    /// Send a note on the channel, or keep it until the channel opens.
    fn note(&mut self, text: String) {
        let Some(v) = &mut self.video else { return };
        let Some(id) = v.channel else {
            if v.outbox.len() < OUTBOX {
                v.outbox.push(text);
            }
            return;
        };
        if let Some(mut ch) = self.rtc.channel(id) {
            match ch.write(false, text.as_bytes()) {
                Ok(true) => {}
                Ok(false) => tracing::debug!("stream: the channel is full; a note was dropped"),
                Err(e) => tracing::debug!(error = %e, "stream: a note was not sent"),
            }
        }
    }

    fn ask_keyframe(&mut self) {
        let Some(mid) = self.video.as_ref().and_then(|v| v.mid) else {
            return;
        };
        if let Some(mut w) = self.rtc.writer(mid)
            && let Err(e) = w.request_keyframe(None, KeyframeRequestKind::Pli)
        {
            tracing::debug!(error = %e, "stream: no keyframe asked for");
        }
    }

    /// The friend's side asked for a keyframe.
    pub(super) fn keyframe_wanted(&mut self) {
        if let Some(Role::Share { flags, .. }) = self.video.as_ref().map(|v| &v.role) {
            flags.keyframe.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn channel_open(&mut self, id: ChannelId, label: &str) {
        let Some(v) = &mut self.video else { return };
        if label != CHANNEL {
            return;
        }
        v.channel = Some(id);
        let mut waiting = std::mem::take(&mut v.outbox);
        if let Role::Share { title, .. } = &v.role {
            waiting.insert(0, serde_json::json!({ "title": title }).to_string());
        }
        for text in waiting {
            self.note(text);
        }
    }

    /// A note from the friend's side.
    pub(super) fn channel_data(&mut self, d: &ChannelData) {
        let Some(v) = &self.video else { return };
        let Ok(note) = serde_json::from_slice::<Value>(&d.data) else {
            tracing::debug!("stream: a note not understood");
            return;
        };
        let flag = |k: &str| note.get(k).and_then(Value::as_bool);
        match &v.role {
            Role::Share { flags, .. } => {
                if let Some(on) = flag("watch") {
                    flags.watched.store(on, Ordering::Relaxed);
                    if on {
                        flags.keyframe.store(true, Ordering::Relaxed);
                    }
                    (v.sink)(StreamEvent::Watching(on));
                }
            }
            Role::Watch { .. } => {
                if let Some(t) = note.get("title").and_then(Value::as_str) {
                    let title: String = t.chars().take(200).collect();
                    (v.sink)(StreamEvent::Title(title));
                }
                if let Some(on) = flag("paused") {
                    (v.sink)(StreamEvent::Paused(on));
                }
            }
        }
    }
}
