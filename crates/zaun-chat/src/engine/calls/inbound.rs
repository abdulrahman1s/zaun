//! The friend's `x.call.*` events (Subscriber.hs `xCallInv`, `xCallOffer`, `xCallAnswer`, `xCallExtra`, `xCallEnd`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{CallSignal, StreamSignal};

use super::line::Over;
use super::state::{Call, Kind, Phase, RING_TTL_MS};
use super::{key, webrtc};
use crate::chat::ChatEvent;
use crate::chat::call::{CallAnswer, CallExtraInfo, CallInvitation, CallMedia, CallOffer};
use crate::chat::ids::CallId;
use crate::chat::version::{CALL_INITIAL_VERSION, SUPPORTED_CALL_VRANGE, VersionRange};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::friend::Feature;

/// A video call without the apps' encryption and without a key: a stream from another Zaun.
fn is_stream(inv: &CallInvitation) -> bool {
    let t = inv.call_type;
    t.media == CallMedia::Video && !t.capabilities.encryption && inv.call_dh_pub_key.is_none()
}

impl<A: AgentPort> Engine<A> {
    /// A blocked friend's invitation is ended at once with their call id; nothing of it is kept.
    pub(crate) fn on_call_event(
        &mut self,
        friend: i64,
        _shared: Option<Vec<u8>>,
        event: ChatEvent,
    ) -> Result<()> {
        if self.friend(friend)?.blocked {
            if let ChatEvent::CallInv { call_id, .. } = &event {
                self.send_end(friend, &call_id.0);
            }
            return Ok(());
        }
        match event {
            ChatEvent::CallInv {
                call_id,
                invitation,
            } => self.on_inv(friend, &call_id, &invitation),
            ChatEvent::CallOffer { call_id, offer } => self.on_offer(friend, &call_id, &offer),
            ChatEvent::CallAnswer { call_id, answer } => self.on_answer(friend, &call_id, &answer),
            ChatEvent::CallExtra { call_id, extra } => self.on_extra(friend, &call_id, &extra),
            ChatEvent::CallEnd { call_id } => self.on_end(friend, &call_id),
            // Only the five are routed here.
            other => {
                self.log
                    .line(format_args!("not a call event: {:?}", other.kind()));
                Ok(())
            }
        }
    }

    fn on_inv(&mut self, friend: i64, id: &CallId, inv: &CallInvitation) -> Result<()> {
        // Subscriber.hs `xCallInv`: `featureRejected CFCalls` when the user's side forbids calls.
        if !self.friend(friend)?.enabled(Feature::Calls).for_contact {
            self.log
                .line(format_args!("x.call.inv: calls are not allowed"));
            return Ok(());
        }
        let peer = inv
            .call_v_range
            .unwrap_or(VersionRange::single(CALL_INITIAL_VERSION));
        let Some(version) = SUPPORTED_CALL_VRANGE.compatible(&peer) else {
            self.log
                .line(format_args!("x.call.inv: incompatible call version"));
            return Ok(());
        };
        let kind = if is_stream(inv) {
            Kind::Stream
        } else {
            Kind::Voice
        };
        let pair = inv
            .call_type
            .capabilities
            .encryption
            .then(key::new_pair)
            .transpose()?;
        let shared = match (&inv.call_dh_pub_key, &pair) {
            (Some(text), Some(own)) => key::parse_public(text)
                .and_then(|k| key::media_key(version, &id.0, &k, &own.private)),
            _ => None,
        };
        // One call per friend: this one replaces the current.
        self.replace_call(friend);
        let now = self.clock.wall_ms();
        self.state.calls.calls.insert(
            friend,
            Call {
                id: id.0.clone(),
                kind,
                phase: Phase::InviteReceived,
                outgoing: false,
                version,
                private: None,
                public: pair.map(|p| p.public),
                key: shared,
                started: now,
                connected: None,
                deadline: Some(now + RING_TTL_MS),
            },
        );
        self.emit(match kind {
            Kind::Voice => Event::Call(CallSignal::Invited {
                friend,
                usable: true,
                key: shared,
            }),
            Kind::Stream => Event::Stream(StreamSignal::Invited(friend)),
        });
        Ok(())
    }

    /// The friend's call when `id` is its id and it is in `phase`; otherwise logged and ignored.
    fn expect(
        &mut self,
        friend: i64,
        id: &CallId,
        what: &str,
        phase: &[Phase],
    ) -> Option<&mut Call> {
        let found = self
            .state
            .calls
            .calls
            .get(&friend)
            .map(|c| (c.id == id.0, c.phase));
        let why = match found {
            None => "no current call",
            Some((false, _)) => "wrong contact or callId",
            Some((true, p)) if !phase.contains(&p) => "wrong call state",
            Some((true, _)) => return self.state.calls.calls.get_mut(&friend),
        };
        self.log.line(format_args!("{what}: {why}"));
        None
    }

    fn on_offer(&mut self, friend: i64, id: &CallId, offer: &CallOffer) -> Result<()> {
        let version = offer.call_version.unwrap_or(CALL_INITIAL_VERSION);
        if !(SUPPORTED_CALL_VRANGE.min..=SUPPORTED_CALL_VRANGE.max).contains(&version) {
            self.log
                .line(format_args!("x.call.offer: unsupported call version"));
            return Ok(());
        }
        let Some(call) = self.expect(friend, id, "x.call.offer", &[Phase::InviteSent]) else {
            return Ok(());
        };
        let shared = match (&offer.call_dh_pub_key, &call.private) {
            (Some(text), Some(own)) => {
                key::parse_public(text).and_then(|k| key::media_key(version, &id.0, &k, own))
            }
            _ => None,
        };
        call.phase = Phase::OfferReceived;
        call.version = version;
        call.key = shared;
        let kind = call.kind;
        let candidates = webrtc::candidates(&offer.rtc_session.rtc_ice_candidates);
        match kind {
            Kind::Voice => {
                let signal = match webrtc::description(&offer.rtc_session) {
                    Some(sdp) => CallSignal::Offer {
                        friend,
                        sdp,
                        candidates,
                        key: shared,
                    },
                    None => CallSignal::Unusable(friend),
                };
                self.emit(Event::Call(signal));
            }
            Kind::Stream => match webrtc::plain_description(&offer.rtc_session) {
                Some(sdp) => self.emit(Event::Stream(StreamSignal::Offer {
                    friend,
                    sdp,
                    candidates,
                })),
                None => self.not_a_stream(friend),
            },
        }
        Ok(())
    }

    fn on_answer(&mut self, friend: i64, id: &CallId, answer: &CallAnswer) -> Result<()> {
        let Some(call) = self.expect(friend, id, "x.call.answer", &[Phase::OfferSent]) else {
            return Ok(());
        };
        call.phase = Phase::Negotiated;
        let kind = call.kind;
        let candidates = webrtc::candidates(&answer.rtc_session.rtc_ice_candidates);
        match kind {
            Kind::Voice => {
                let signal = match webrtc::description(&answer.rtc_session) {
                    Some(sdp) => CallSignal::Answer {
                        friend,
                        sdp,
                        candidates,
                    },
                    None => CallSignal::Unusable(friend),
                };
                self.emit(Event::Call(signal));
            }
            Kind::Stream => match webrtc::plain_description(&answer.rtc_session) {
                Some(sdp) => self.emit(Event::Stream(StreamSignal::Answer {
                    friend,
                    sdp,
                    candidates,
                })),
                None => self.not_a_stream(friend),
            },
        }
        Ok(())
    }

    fn on_extra(&mut self, friend: i64, id: &CallId, extra: &CallExtraInfo) -> Result<()> {
        let phases = [Phase::OfferReceived, Phase::Negotiated];
        let Some(call) = self.expect(friend, id, "x.call.extra", &phases) else {
            return Ok(());
        };
        let kind = call.kind;
        let candidates = webrtc::candidates(&extra.rtc_extra_info.rtc_ice_candidates);
        self.emit(match kind {
            Kind::Voice => Event::Call(CallSignal::Extra { friend, candidates }),
            Kind::Stream => Event::Stream(StreamSignal::Extra { friend, candidates }),
        });
        Ok(())
    }

    /// The caller hanging up while the user's call still rings is a decline.
    fn on_end(&mut self, friend: i64, id: &CallId) -> Result<()> {
        let all = [
            Phase::InviteSent,
            Phase::InviteReceived,
            Phase::OfferSent,
            Phase::OfferReceived,
            Phase::Negotiated,
        ];
        if self.expect(friend, id, "x.call.end", &all).is_none() {
            return Ok(());
        }
        let Some(call) = self.state.calls.calls.remove(&friend) else {
            return Ok(());
        };
        let declined = call.phase == Phase::InviteSent;
        let over = if declined {
            Over::Declined
        } else {
            Over::Ended
        };
        // The signal first, as the old engine did: the call ends before its line is shown.
        self.emit(match (call.kind, declined) {
            (Kind::Voice, true) => Event::Call(CallSignal::Declined(friend)),
            (Kind::Voice, false) => Event::Call(CallSignal::Ended(friend)),
            (Kind::Stream, _) => Event::Stream(StreamSignal::Ended(friend)),
        });
        self.close_call(friend, &call, over);
        Ok(())
    }

    /// Not plain SDP: not a stream from Zaun after all.
    fn not_a_stream(&mut self, friend: i64) {
        if let Some(call) = self.state.calls.calls.remove(&friend) {
            self.send_end(friend, &call.id);
        }
        self.emit(Event::Stream(StreamSignal::Ended(friend)));
    }
}
