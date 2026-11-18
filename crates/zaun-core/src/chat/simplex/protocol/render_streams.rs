//! Stream requests become the engine's `/_call …` commands with `media: "video"` (ADR-0036).
//! The engine holds one call per friend, so the codec remembers which friends' calls are
//! streams ([`super::respond_streams::Streams`]) and routes their events to the stream flow.
use super::*;
use crate::chat::media::{StreamRequest, StreamSignal};

/// A stream: video, without the apps' own media encryption (as for calls, ADR-0035).
fn stream_type() -> Value {
    serde_json::json!({ "media": "video", "capabilities": { "encryption": false } })
}

fn session(sdp: &str) -> Value {
    serde_json::json!({ "rtcSession": sdp, "rtcIceCandidates": "[]" })
}

impl Codec {
    pub(super) fn render_stream(&mut self, request: StreamRequest, out: &mut Output) {
        let (friend, quiet, cmd) = match request {
            StreamRequest::Invite(f) => {
                self.streams.begin(f, true);
                (f, false, format!("/_call invite @{f} {}", stream_type()))
            }
            StreamRequest::Offer { friend, sdp } => {
                let offer =
                    serde_json::json!({ "callType": stream_type(), "rtcSession": session(&sdp) });
                (friend, false, format!("/_call offer @{friend} {offer}"))
            }
            StreamRequest::Answer { friend, sdp } => (
                friend,
                false,
                format!("/_call answer @{friend} {}", session(&sdp)),
            ),
            StreamRequest::Connected(f) => (f, true, format!("/_call status @{f} connected")),
            StreamRequest::End(f) => {
                // A stream already gone (the friend ended it first) needs no word.
                if !self.streams.end(f) {
                    return;
                }
                (f, true, format!("/_call end @{f}"))
            }
        };
        self.send(out, Pending::Stream { friend, quiet }, cmd);
    }

    /// A voice call with `friend` is about to take the engine's call slot: their stream is
    /// over (the engine replaces it without a word).
    pub(super) fn stream_replaced(&mut self, friend: i64, out: &mut Output) {
        if self.streams.end(friend) {
            out.events
                .push(ChatEvent::Stream(StreamSignal::Ended(friend)));
        }
    }
}
