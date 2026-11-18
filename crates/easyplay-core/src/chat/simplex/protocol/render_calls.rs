//! Call requests become the engine's `/_call …` commands (ADR-0035). Descriptions and
//! candidates go in the apps' form (`webrtc`), so a call works with the official apps too.
use super::webrtc;
use super::*;
use crate::chat::media::CallRequest;

/// What EasyPlay calls with: voice, with the apps' frame encryption. The engine agrees a key
/// when the friend's app asks for it too (`sharedKey`), and the media then uses it.
fn call_type() -> Value {
    serde_json::json!({ "media": "audio", "capabilities": { "encryption": true } })
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
                let offer = serde_json::json!({
                    "callType": call_type(),
                    "rtcSession": webrtc::session(&sdp, "offer"),
                });
                (friend, false, format!("/_call offer @{friend} {offer}"))
            }
            CallRequest::Answer { friend, sdp } => (
                friend,
                false,
                format!(
                    "/_call answer @{friend} {}",
                    webrtc::session(&sdp, "answer")
                ),
            ),
            CallRequest::Extra { friend, candidates } => {
                let list = webrtc::extra_candidates(&candidates);
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
