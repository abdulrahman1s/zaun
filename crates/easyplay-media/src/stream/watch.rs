//! Taking a friend's stream (ADR-0036): the session writes the offer at once; the decoder
//! thread turns the units it is handed into page frames while the user watches, publishing
//! only the latest (the frame slot drops what the page did not draw).
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel, sync_channel};
use std::time::Duration;

use easyplay_core::chat::media::{
    FrameSlot, Negotiation, StreamMedia, StreamSink, VideoFrame, WatchConfig,
};
use easyplay_core::error::Result;

use super::media_config;
use crate::clock::now;
use crate::factory::RtcMedia;
use crate::session::Inbox;
use crate::session::video::{self, Role, VideoMsg, VideoPart, stream_media_sink};
use crate::video::codec::H264Decoder;

/// Units waiting for the decoder (more are dropped and a keyframe asked for).
const DECODER_QUEUE: usize = 4;
/// At most one keyframe request this often.
const KEYFRAME_EVERY: Duration = Duration::from_secs(1);

pub(super) fn start(
    m: &RtcMedia,
    config: WatchConfig,
    sink: StreamSink,
) -> Result<Box<dyn StreamMedia>> {
    let (tx, rx) = channel();
    let watching = Arc::new(AtomicBool::new(false));
    let (units, unit_rx) = sync_channel(DECODER_QUEUE);
    let part = VideoPart::new(
        Role::Watch {
            watching: Arc::clone(&watching),
            decoder: units,
        },
        Arc::clone(&sink),
    );
    let fail = |e: std::io::Error| easyplay_core::err!(ChtStreamFailed).with_source(e);
    let (frames, w, keyframes) = (config.frames.clone(), Arc::clone(&watching), tx.clone());
    std::thread::Builder::new()
        .name("watch-decoder".into())
        .spawn(move || decode(&unit_rx, &frames, &w, &keyframes))
        .map_err(fail)?;
    let mc = media_config(Negotiation::Offer, config.ice, config.relay_only);
    video::spawn(
        mc,
        stream_media_sink(sink),
        m.options(),
        (tx.clone(), rx),
        part,
    )
    .map_err(fail)?;
    Ok(Box::new(RtcWatch {
        tx,
        watching,
        frames: config.frames,
    }))
}

/// A friend's stream as the flow holds it. Dropping it lets the stream go.
struct RtcWatch {
    tx: Sender<Inbox>,
    watching: Arc<AtomicBool>,
    frames: FrameSlot,
}

impl RtcWatch {
    fn send(&self, msg: Inbox) {
        // The session may have ended already (it reported why).
        let _ = self.tx.send(msg);
    }
}

impl StreamMedia for RtcWatch {
    fn offer(&self, _sdp: String) {
        tracing::debug!("stream: an offer for a watched stream (this side offers)");
    }

    fn answer(&self, sdp: String) {
        self.send(Inbox::Answer(sdp));
    }

    fn candidates(&self, candidates: Vec<String>) {
        self.send(Inbox::Candidates(candidates));
    }

    fn watch(&self, on: bool) {
        if self.watching.swap(on, Ordering::Relaxed) == on {
            return;
        }
        let note = serde_json::json!({ "watch": on }).to_string();
        self.send(Inbox::Video(VideoMsg::Note(note)));
        if on {
            self.send(Inbox::Video(VideoMsg::Keyframe));
        } else {
            self.frames.clear();
        }
    }

    // The stream carries no sound yet (ADR-0036): nothing to turn off.
    fn mute(&self, _on: bool) {}

    fn set_quality(&self, _max_height: Option<u32>, _fps: u32) {
        tracing::debug!("stream: a quality for a watched stream (the sharer chooses)");
    }
}

impl Drop for RtcWatch {
    fn drop(&mut self) {
        self.watching.store(false, Ordering::Relaxed);
        self.send(Inbox::Stop);
    }
}

/// The decoder thread: ends when the session drops its side of the queue.
fn decode(
    units: &Receiver<Arc<[u8]>>,
    frames: &FrameSlot,
    watching: &AtomicBool,
    session: &Sender<Inbox>,
) {
    let mut decoder = match H264Decoder::new() {
        Ok(d) => d,
        Err(e) => return tracing::warn!(error = %e, "stream: no H.264 decoder"),
    };
    let mut spare = VideoFrame::black(2, 2);
    let mut asked = None;
    while let Ok(unit) = units.recv() {
        if !watching.load(Ordering::Relaxed) {
            continue;
        }
        match decoder.decode(&unit, &mut spare) {
            Ok(true) => {
                let next = frames.publish(std::mem::replace(&mut spare, VideoFrame::black(2, 2)));
                if let Some(old) = next {
                    spare = old;
                }
            }
            Ok(false) => {}
            Err(e) => {
                tracing::debug!(error = %e, "stream: a unit did not decode");
                if asked.is_none_or(|t: std::time::Instant| t.elapsed() >= KEYFRAME_EVERY) {
                    asked = Some(now());
                    let _ = session.send(Inbox::Video(VideoMsg::Keyframe));
                }
            }
        }
    }
}
