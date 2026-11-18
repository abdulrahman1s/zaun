//! The engine's call events become [`CallSignal`]s (ADR-0035), and the call's line in the
//! conversation is shown once the call is over.
//!
//! - An invitation is usable when it is a voice call without the apps' own encryption and
//!   without a shared key; anything else is another app's call and is let go as missed.
//! - An offer or answer whose session is not plain SDP is [`CallSignal::Unusable`].
//! - A blocked friend's call is ended at once and never reported.
//! - A call item is shown only when the call is over (missed, declined, ended): while it
//!   rings or runs it is the call card's, not a line. Its first final state arrives as an
//!   update of an item the page never saw, so it is reported as a new message.
//! - **Declined.** The engine's own decline (`/_call reject`) never tells the caller, so a
//!   decline is sent as `/_call end`, which ends the call at once on both sides; the engine
//!   then marks both lines "missed". The codec remembers the decline (on the callee's side,
//!   from the request; on the caller's, from a `callEnded` that arrives while the call still
//!   rings) and reports that call's line as declined, and on the callee's side as read
//!   (`/_read chat items`). The engine's database keeps "missed", so after a restart the
//!   history reads "Missed call" for these.
use std::collections::{HashSet, VecDeque};

use super::respond::Answer;
use super::*;
use crate::chat::media::{CallRequest, CallSignal};

/// Call items reported as new lately (an item is final once, but the engine may say so more
/// than once): at most this many are remembered.
const SHOWN_CALLS: usize = 16;

/// The call items reported as new, newest last ([`SHOWN_CALLS`] at most).
#[derive(Debug, Default)]
pub(super) struct ShownCalls {
    shown: VecDeque<(ChatRef, i64)>,
    /// Friends the user is ringing (no offer yet): their hang-up is a decline. One entry per
    /// friend at most; removed when the call is answered or ended.
    ringing: HashSet<i64>,
    /// Friends whose current call ended as declined; removed when its line is reported.
    declined: HashSet<i64>,
}

impl ShownCalls {
    /// The user rings `friend`.
    pub fn ringing(&mut self, friend: i64) {
        self.ringing.insert(friend);
    }

    /// The call with `friend` stopped ringing (answered, or ended by the user).
    pub fn settled(&mut self, friend: i64) {
        self.ringing.remove(&friend);
    }

    /// The call with `friend` ends as declined.
    pub fn declined(&mut self, friend: i64) {
        self.ringing.remove(&friend);
        self.declined.insert(friend);
    }
}

impl Codec {
    pub(super) fn on_call_answer(
        &mut self,
        friend: i64,
        quiet: bool,
        a: Answer<'_>,
        out: &mut Output,
    ) {
        let Some(e) = a.error else { return };
        tracing::info!(friend, error = %short(e), "chat: a call command failed");
        if !quiet {
            out.events.push(ChatEvent::Call(CallSignal::Refused {
                friend,
                detail: short(e),
            }));
        }
    }

    pub(super) fn on_call_event(&mut self, ty: &str, r: &Value, out: &mut Output) {
        let inv = r.get("callInvitation");
        let contact = inv.unwrap_or(r).get("contact");
        let Some(friend) = contact
            .and_then(|c| c.get("contactId"))
            .and_then(Value::as_i64)
        else {
            return;
        };
        if self.blocked.contains(&friend) {
            if ty == "callInvitation" {
                self.render_call(CallRequest::End(friend), out);
            }
            return;
        }
        let signal = match ty {
            "callInvitation" => CallSignal::Invited {
                friend,
                usable: inv.is_some_and(usable_invitation),
            },
            "callOffer" => {
                self.shown_calls.settled(friend);
                session_signal(friend, r.get("offer"), true)
            }
            "callAnswer" => session_signal(friend, r.get("answer"), false),
            "callExtraInfo" => CallSignal::Extra {
                friend,
                candidates: candidates(r.get("extraInfo")),
            },
            "callEnded" if self.shown_calls.ringing.contains(&friend) => {
                self.shown_calls.declined(friend);
                CallSignal::Declined(friend)
            }
            "callEnded" => CallSignal::Ended(friend),
            _ => return,
        };
        out.events.push(ChatEvent::Call(signal));
    }

    /// A `chatItemUpdated` of a call item: reported as a new message once it is final.
    pub(super) fn on_call_item(&mut self, r: &Value, out: &mut Output) {
        let Some(mut m) = r.get("chatItem").and_then(|a| self.parse_a_item(a)) else {
            return;
        };
        if let ChatRef::Direct(f) = m.chat
            && self.streams.hides_item(f, m.id)
        {
            return;
        }
        let key = (m.chat, m.id);
        let calls = &mut self.shown_calls;
        if calls.shown.contains(&key) {
            return;
        }
        if calls.shown.len() == SHOWN_CALLS {
            calls.shown.pop_front();
        }
        calls.shown.push_back(key);
        if let (ChatRef::Direct(f), MessageBody::Call(rec)) = (m.chat, &mut m.body)
            && rec.kind == crate::model::CallKind::Missed
            && calls.declined.remove(&f)
        {
            rec.kind = crate::model::CallKind::Declined;
            if m.unread {
                m.unread = false;
                let cmd = format!("/_read chat items @{f} {}", m.id);
                self.send(out, Pending::Quiet, cmd);
            }
        }
        out.events.push(ChatEvent::Messages(vec![m]));
    }
}

/// Whether a `chatItemUpdated` is about a call item.
pub(super) fn is_call_item(r: &Value) -> bool {
    matches!(
        str_at(r, &["chatItem", "chatItem", "content", "type"]),
        Some("sndCall" | "rcvCall")
    )
}

fn usable_invitation(inv: &Value) -> bool {
    let ct = inv.get("callType");
    let audio = ct.and_then(|c| c.get("media")).and_then(Value::as_str) == Some("audio");
    let encrypted = ct
        .and_then(|c| c.get("capabilities"))
        .and_then(|c| c.get("encryption"))
        .and_then(Value::as_bool)
        != Some(false);
    let keyed = inv.get("sharedKey").is_some_and(|k| !k.is_null());
    audio && !encrypted && !keyed
}

/// An offer or answer (`WebRTCSession`).
fn session_signal(friend: i64, s: Option<&Value>, offer: bool) -> CallSignal {
    let sdp = s
        .and_then(|s| s.get("rtcSession"))
        .and_then(Value::as_str)
        .filter(|t| t.starts_with("v=0"));
    let Some(sdp) = sdp else {
        return CallSignal::Unusable(friend);
    };
    let sdp = sdp.to_owned();
    let candidates = candidates(s);
    if offer {
        CallSignal::Offer {
            friend,
            sdp,
            candidates,
        }
    } else {
        CallSignal::Answer {
            friend,
            sdp,
            candidates,
        }
    }
}

/// `rtcIceCandidates`: a JSON list (as text) of `candidate:…` strings, or of objects with a
/// `candidate` field. Anything else (the apps' compressed form) gives none.
pub(super) fn candidates(s: Option<&Value>) -> Vec<String> {
    let Some(text) = s
        .and_then(|s| s.get("rtcIceCandidates"))
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| c.as_str().or_else(|| c.get("candidate")?.as_str()))
        .map(|c| c.strip_prefix("a=").unwrap_or(c))
        .filter(|c| c.starts_with("candidate:"))
        .map(str::to_owned)
        .collect()
}
