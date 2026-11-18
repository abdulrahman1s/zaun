//! Call requests become the engine's `/_call …` commands (ADR-0035). The session strings
//! carry plain SDP with the candidates inside it, so `rtcIceCandidates` is an empty JSON
//! list; the official apps put compressed JSON there instead, which is how a call from one
//! of them is told apart.
use super::*;
use crate::chat::media::CallRequest;

/// What EasyPlay calls with: voice, without the apps' own media encryption (the media is
/// DTLS-SRTP end to end; the apps' extra key exchange is theirs). An invitation that asks
/// for it comes from another app.
fn call_type() -> Value {
    serde_json::json!({ "media": "audio", "capabilities": { "encryption": false } })
}

/// A `WebRTCSession`: the SDP and an empty candidate list.
fn session(sdp: &str) -> Value {
    serde_json::json!({ "rtcSession": sdp, "rtcIceCandidates": "[]" })
}

impl Codec {
    pub(super) fn render_call(&mut self, request: CallRequest, out: &mut Output) {
        // A failed hang-up, decline or status is harmless (the call is already gone).
        let (friend, quiet, cmd) = match request {
            CallRequest::Invite(f) => {
                self.stream_replaced(f, out);
                self.shown_calls.ringing(f);
                (f, false, format!("/_call invite @{f} {}", call_type()))
            }
            CallRequest::Offer { friend, sdp } => {
                let offer =
                    serde_json::json!({ "callType": call_type(), "rtcSession": session(&sdp) });
                (friend, false, format!("/_call offer @{friend} {offer}"))
            }
            CallRequest::Answer { friend, sdp } => (
                friend,
                false,
                format!("/_call answer @{friend} {}", session(&sdp)),
            ),
            CallRequest::Extra { friend, candidates } => {
                let list = Value::from(candidates).to_string();
                let extra = serde_json::json!({ "rtcIceCandidates": list });
                (friend, true, format!("/_call extra @{friend} {extra}"))
            }
            CallRequest::Connected(f) => (f, true, format!("/_call status @{f} connected")),
            CallRequest::Reject(f) => {
                self.shown_calls.settled(f);
                (f, true, format!("/_call reject @{f}"))
            }
            // Ending the invitation is what tells the caller at once (`/_call reject` does not).
            CallRequest::Decline(f) => {
                self.shown_calls.declined(f);
                (f, true, format!("/_call end @{f}"))
            }
            CallRequest::End(f) => {
                self.shown_calls.settled(f);
                (f, true, format!("/_call end @{f}"))
            }
        };
        self.send(out, Pending::Call { friend, quiet }, cmd);
    }
}
