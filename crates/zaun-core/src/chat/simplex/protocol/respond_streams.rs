//! The engine's call events for streams become [`StreamSignal`]s (ADR-0036).
//!
//! - An invitation with `media: "video"` and without the apps' own encryption is a stream
//!   from another Zaun; anything else stays with the call flow.
//! - The engine holds one call per friend: a voice invitation (theirs or the user's) for a
//!   friend whose stream runs ends that stream first.
//! - A stream's call item never shows: not while it runs, not once over. It is known by its
//!   number, seen when it appears while the stream is open (the engine's item says nothing
//!   about video; after a restart the history shows past streams as calls).
//! - A blocked friend's stream is ended at once and never reported.
use std::collections::{HashMap, VecDeque};

use super::respond::Answer;
use super::webrtc::candidates;
use super::*;
use crate::chat::media::{StreamRequest, StreamSignal};

/// Streams' call items whose final state may still come: at most this many are remembered.
const ENDED_ITEMS: usize = 16;

/// Friends whose engine call is a stream, and the call items of streams.
#[derive(Debug, Default)]
pub(super) struct Streams {
    /// By friend: `true` = the user shares, `false` = the friend does. One entry per friend
    /// (the engine's call slot); removed when the stream ends.
    live: HashMap<i64, bool>,
    /// The call item of each open stream (friend → item), once it appeared.
    items: HashMap<i64, i64>,
    /// Items of streams that ended, newest last ([`ENDED_ITEMS`] at most).
    ended: VecDeque<(i64, i64)>,
}

impl Streams {
    pub fn begin(&mut self, friend: i64, sharing: bool) {
        self.items.remove(&friend);
        self.live.insert(friend, sharing);
    }

    /// The stream with `friend` is over; whether there was one.
    pub fn end(&mut self, friend: i64) -> bool {
        let was = self.live.remove(&friend).is_some();
        if let Some(item) = self.items.remove(&friend) {
            if self.ended.len() == ENDED_ITEMS {
                self.ended.pop_front();
            }
            self.ended.push_back((friend, item));
        }
        was
    }

    /// A request for a stream was refused: no stream, and no item to hide.
    pub fn forget(&mut self, friend: i64) {
        self.live.remove(&friend);
        self.items.remove(&friend);
    }

    pub fn is_stream(&self, friend: i64) -> bool {
        self.live.contains_key(&friend)
    }

    /// A call item with `friend` appeared: an open stream's first one is the stream's.
    pub fn note_item(&mut self, friend: i64, item: i64) {
        if self.live.contains_key(&friend) {
            self.items.entry(friend).or_insert(item);
        }
    }

    /// Whether call item `item` with `friend` is a stream's (an ended one's is taken once).
    pub fn hides_item(&mut self, friend: i64, item: i64) -> bool {
        if self.items.get(&friend) == Some(&item) {
            return true;
        }
        let Some(i) = self.ended.iter().position(|e| *e == (friend, item)) else {
            return false;
        };
        self.ended.remove(i);
        true
    }
}

impl Codec {
    /// New items: a call item with a friend whose stream is open belongs to that stream.
    pub(super) fn note_stream_items(&mut self, r: &Value) {
        let Some(items) = r.get("chatItems").and_then(Value::as_array) else {
            return;
        };
        for it in items {
            let (Some(friend), Some(item)) = (
                it.get("chatInfo")
                    .and_then(|c| c.get("contact"))
                    .and_then(|c| c.get("contactId"))
                    .and_then(Value::as_i64),
                it.get("chatItem")
                    .and_then(|c| c.get("meta"))
                    .and_then(|m| m.get("itemId"))
                    .and_then(Value::as_i64),
            ) else {
                continue;
            };
            if matches!(
                str_at(it, &["chatItem", "content", "type"]),
                Some("sndCall" | "rcvCall")
            ) {
                self.streams.note_item(friend, item);
            }
        }
    }

    pub(super) fn on_stream_answer(
        &mut self,
        friend: i64,
        quiet: bool,
        a: Answer<'_>,
        out: &mut Output,
    ) {
        let Some(e) = a.error else { return };
        tracing::info!(friend, error = %short(e), "chat: a stream command failed");
        if !quiet {
            self.streams.forget(friend);
            out.events.push(ChatEvent::Stream(StreamSignal::Refused {
                friend,
                detail: short(e),
            }));
        }
    }

    /// A call event that belongs to a stream becomes a stream signal; `false`: the call
    /// flow's.
    pub(super) fn on_stream_event(&mut self, ty: &str, r: &Value, out: &mut Output) -> bool {
        let inv = r.get("callInvitation");
        let Some(friend) = inv
            .unwrap_or(r)
            .get("contact")
            .and_then(|c| c.get("contactId"))
            .and_then(Value::as_i64)
        else {
            return false;
        };
        if ty == "callInvitation" {
            // Whatever it is, it takes the place of a stream with this friend.
            self.stream_replaced(friend, out);
            if !inv.is_some_and(stream_invitation) {
                return false;
            }
            self.streams.begin(friend, false);
            if self.blocked.contains(&friend) {
                self.render_stream(StreamRequest::End(friend), out);
                return true;
            }
            out.events
                .push(ChatEvent::Stream(StreamSignal::Invited(friend)));
            return true;
        }
        if !self.streams.is_stream(friend) {
            return false;
        }
        let signal = match ty {
            "callOffer" => session(friend, r.get("offer"), true),
            "callAnswer" => session(friend, r.get("answer"), false),
            "callExtraInfo" => Some(StreamSignal::Extra {
                friend,
                candidates: candidates(r.get("extraInfo")),
            }),
            "callEnded" => {
                self.streams.end(friend);
                Some(StreamSignal::Ended(friend))
            }
            _ => None,
        };
        let signal = signal.unwrap_or_else(|| {
            // Not plain SDP: not an Zaun stream after all.
            self.render_stream(StreamRequest::End(friend), out);
            StreamSignal::Ended(friend)
        });
        out.events.push(ChatEvent::Stream(signal));
        true
    }
}

/// A video call from another Zaun: video, no media encryption of the apps', no key.
fn stream_invitation(inv: &Value) -> bool {
    let ct = inv.get("callType");
    let video = ct.and_then(|c| c.get("media")).and_then(Value::as_str) == Some("video");
    let plain = ct
        .and_then(|c| c.get("capabilities"))
        .and_then(|c| c.get("encryption"))
        .and_then(Value::as_bool)
        == Some(false);
    let keyed = inv.get("sharedKey").is_some_and(|k| !k.is_null());
    video && plain && !keyed
}

/// An offer or answer (`WebRTCSession`) with plain SDP.
fn session(friend: i64, s: Option<&Value>, offer: bool) -> Option<StreamSignal> {
    let sdp = s
        .and_then(|s| s.get("rtcSession"))
        .and_then(Value::as_str)
        .filter(|t| t.starts_with("v=0"))?
        .to_owned();
    let candidates = candidates(s);
    Some(if offer {
        StreamSignal::Offer {
            friend,
            sdp,
            candidates,
        }
    } else {
        StreamSignal::Answer {
            friend,
            sdp,
            candidates,
        }
    })
}
