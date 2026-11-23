//! The user's call requests (Commands.hs `APISendCallInvitation`, `APISendCallOffer`, `APISendCallAnswer`, `APISendCallExtraInfo`, `APIEndCall`, `APIRejectCall`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{CallRequest, CallSignal};

use super::line::Over;
use super::state::{Call, Kind, Phase, RING_TTL_MS};
use super::{key, webrtc};
use crate::chat::call::{
    CallAnswer, CallCapabilities, CallExtraInfo, CallInvitation, CallMedia, CallOffer, CallType,
    WebRtcExtraInfo,
};
use crate::chat::ids::CallId;
use crate::chat::version::{CALL_INITIAL_VERSION, SUPPORTED_CALL_VRANGE};
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::friend::Feature;

/// What chat messages are sized for on a post-quantum connection.
const PQ: bool = true;

/// What Zaun calls with: voice with the apps' frame encryption, or a stream's video without.
pub(super) fn call_type(kind: Kind) -> CallType {
    match kind {
        Kind::Voice => CallType {
            media: CallMedia::Audio,
            capabilities: CallCapabilities { encryption: true },
        },
        Kind::Stream => CallType {
            media: CallMedia::Video,
            capabilities: CallCapabilities { encryption: false },
        },
    }
}

pub(super) fn session_for(kind: Kind, sdp: &str, what: &str) -> crate::chat::call::WebRtcSession {
    match kind {
        Kind::Voice => webrtc::session(sdp, what),
        Kind::Stream => webrtc::plain_session(sdp),
    }
}

impl<A: AgentPort> Engine<A> {
    /// An `Err` of the first three is reported to the call as refused; a hang-up, decline or
    /// status that fails is harmless (the call is already gone) and only logged.
    pub(crate) fn call(&mut self, request: CallRequest) -> Result<()> {
        let (friend, loud, r) = match request {
            CallRequest::Invite(f) => (f, true, self.place(f, Kind::Voice)),
            CallRequest::Offer { friend, sdp } => {
                (friend, true, self.offer(friend, Kind::Voice, &sdp))
            }
            CallRequest::Answer { friend, sdp } => {
                (friend, true, self.answer(friend, Kind::Voice, &sdp))
            }
            CallRequest::Extra { friend, candidates } => {
                (friend, false, self.extra(friend, &candidates))
            }
            CallRequest::Connected(f) => (f, false, self.connected(f, Kind::Voice)),
            CallRequest::Reject(f) => (f, false, self.reject(f)),
            CallRequest::Decline(f) => {
                (f, false, self.hang_up(f, Some(Kind::Voice), Over::Declined))
            }
            CallRequest::End(f) => (f, false, self.hang_up(f, Some(Kind::Voice), Over::Ended)),
        };
        if let Err(e) = r {
            self.log
                .line(format_args!("a call request failed: {}", e.detail));
            if loud {
                self.emit(Event::Call(CallSignal::Refused {
                    friend,
                    detail: e.detail,
                }));
            }
        }
        Ok(())
    }

    pub(super) fn send_call_event(&mut self, friend: i64, event: ChatEvent) -> Result<()> {
        let f = self.friend(friend)?;
        let body = encode_chat_message(&ChatMessage::new(event)?, PQ)?;
        self.agent.send(&mut self.store, &f.conn, body)?;
        Ok(())
    }

    /// `x.call.end` for `id`, whatever state we hold: a failure is only logged.
    pub(super) fn send_end(&mut self, friend: i64, id: &[u8]) {
        let event = ChatEvent::CallEnd {
            call_id: CallId(id.to_vec()),
        };
        if let Err(e) = self.send_call_event(friend, event) {
            self.log
                .line(format_args!("x.call.end was not sent: {}", e.detail));
        }
    }

    /// `APISendCallInvitation`; one call per friend, so a call or stream still there is replaced.
    pub(super) fn place(&mut self, friend: i64, kind: Kind) -> Result<()> {
        let f = self.friend(friend)?;
        if !f.reachable() {
            return Err(fail(Code::CallFailed, "not connected"));
        }
        if !f.enabled(Feature::Calls).for_user {
            return Err(fail(
                Code::CallFailed,
                "calls are not allowed with this friend",
            ));
        }
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|_| fail(Code::CallFailed, "no random"))?;
        let ct = call_type(kind);
        let pair = ct.capabilities.encryption.then(key::new_pair).transpose()?;
        let invitation = CallInvitation {
            call_type: ct,
            call_dh_pub_key: pair.as_ref().map(|p| key::public_b64(&p.public)),
            call_v_range: Some(SUPPORTED_CALL_VRANGE),
        };
        let call_id = CallId(id.to_vec());
        self.send_call_event(
            friend,
            ChatEvent::CallInv {
                call_id,
                invitation,
            },
        )?;
        self.replace_call(friend);
        let now = self.clock.wall_ms();
        self.state.calls.calls.insert(
            friend,
            Call {
                id: id.to_vec(),
                kind,
                phase: Phase::InviteSent,
                outgoing: true,
                version: CALL_INITIAL_VERSION,
                private: pair.map(|p| p.private),
                public: None,
                key: None,
                started: now,
                connected: None,
                deadline: Some(now + RING_TTL_MS),
            },
        );
        Ok(())
    }

    /// `APISendCallOffer`: the callee answers the invitation with its description.
    pub(super) fn offer(&mut self, friend: i64, kind: Kind, sdp: &str) -> Result<()> {
        let call = self.current(friend, kind, Phase::InviteReceived)?;
        let ct = call_type(kind);
        let offer = CallOffer {
            call_type: ct,
            rtc_session: session_for(kind, sdp, "offer"),
            call_dh_pub_key: call
                .public
                .as_ref()
                .filter(|_| ct.capabilities.encryption)
                .map(key::public_b64),
            call_version: Some(call.version),
        };
        let call_id = CallId(call.id.clone());
        self.send_call_event(friend, ChatEvent::CallOffer { call_id, offer })?;
        self.set_phase(friend, Phase::OfferSent);
        Ok(())
    }

    /// `APISendCallAnswer`: the caller answers the offer.
    pub(super) fn answer(&mut self, friend: i64, kind: Kind, sdp: &str) -> Result<()> {
        let call = self.current(friend, kind, Phase::OfferReceived)?;
        let answer = CallAnswer {
            rtc_session: session_for(kind, sdp, "answer"),
        };
        let call_id = CallId(call.id.clone());
        self.send_call_event(friend, ChatEvent::CallAnswer { call_id, answer })?;
        self.set_phase(friend, Phase::Negotiated);
        Ok(())
    }

    /// `APISendCallExtraInfo`: only after an offer was sent or the call negotiated.
    fn extra(&mut self, friend: i64, candidates: &[String]) -> Result<()> {
        let call = self.state.calls.calls.get(&friend);
        let Some(call) = call.filter(|c| {
            c.kind == Kind::Voice && matches!(c.phase, Phase::OfferSent | Phase::Negotiated)
        }) else {
            return Err(fail(Code::CallFailed, "wrong call state"));
        };
        let call_id = CallId(call.id.clone());
        let extra = CallExtraInfo {
            rtc_extra_info: WebRtcExtraInfo {
                rtc_session: None,
                rtc_ice_candidates: webrtc::extra_candidates(candidates),
            },
        };
        self.send_call_event(friend, ChatEvent::CallExtra { call_id, extra })
    }

    /// `APICallStatus connected`: from here the call's duration counts, and it no longer expires.
    pub(super) fn connected(&mut self, friend: i64, kind: Kind) -> Result<()> {
        let now = self.clock.wall_ms();
        let call = self.state.calls.calls.get_mut(&friend);
        let Some(call) = call.filter(|c| c.kind == kind) else {
            return Err(fail(Code::CallFailed, "no call"));
        };
        call.connected.get_or_insert(now);
        call.deadline = None;
        Ok(())
    }

    /// `APIRejectCall`: nothing is sent; the line reads declined and read.
    fn reject(&mut self, friend: i64) -> Result<()> {
        self.current(friend, Kind::Voice, Phase::InviteReceived)?;
        if let Some(call) = self.state.calls.calls.remove(&friend) {
            self.close_call(friend, &call, Over::Declined);
        }
        Ok(())
    }

    /// `APIEndCall` (and a decline, which tells the caller the same way): `x.call.end`, the
    /// call removed, its line written. With `only`, a call of the other kind is left alone.
    pub(super) fn hang_up(&mut self, friend: i64, only: Option<Kind>, over: Over) -> Result<()> {
        if let Some(only) = only
            && self.state.calls.calls.get(&friend).map(|c| c.kind) != Some(only)
        {
            return Ok(());
        }
        let Some(call) = self.state.calls.calls.remove(&friend) else {
            return Ok(());
        };
        self.send_end(friend, &call.id);
        self.close_call(friend, &call, over);
        Ok(())
    }

    /// The call with `friend` when it is of `kind` and in `phase`.
    fn current(&self, friend: i64, kind: Kind, phase: Phase) -> Result<&Call> {
        self.state
            .calls
            .calls
            .get(&friend)
            .filter(|c| c.kind == kind && c.phase == phase)
            .ok_or_else(|| fail(Code::CallFailed, "wrong call state"))
    }

    pub(super) fn set_phase(&mut self, friend: i64, phase: Phase) {
        if let Some(c) = self.state.calls.calls.get_mut(&friend) {
            c.phase = phase;
        }
    }

    /// A new call takes the friend's slot: the old one's line is written, and a stream there
    /// is reported over.
    pub(super) fn replace_call(&mut self, friend: i64) {
        let Some(old) = self.state.calls.calls.remove(&friend) else {
            return;
        };
        self.close_call(friend, &old, Over::Ended);
        if old.kind == Kind::Stream {
            self.emit(Event::Stream(zaun_chat_wire::model::StreamSignal::Ended(
                friend,
            )));
        }
    }
}
