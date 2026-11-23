//! Calls (codec: tests_calls.rs): signals, the call key, the call's line, the ringing timeout.

use std::time::Duration;

use zaun_chat_wire::model::{
    CallKind, CallRecord, CallRequest, CallSignal, ChatMessage, ChatRef, MessageAuthor, MessageBody,
};
use zaun_chat_wire::{Event, Request};

use super::{T, messages};
use crate::chat::call::{
    CallAnswer, CallCapabilities, CallExtraInfo, CallInvitation, CallMedia, CallOffer, CallType,
    WebRtcExtraInfo, WebRtcSession,
};
use crate::chat::ids::CallId;
use crate::chat::version::{SUPPORTED_CALL_VRANGE, VersionRange};
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::crypto::{Alg, KeyPair};
use crate::engine::agent_port::ConnId;
use crate::engine::calls::{media_key, public_b64};

pub(super) const SDP: &str = "v=0\r\no=- 1 2 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\na=candidate:1 1 udp 1 203.0.113.9 9 typ relay\r\n";
pub(super) const CAND: &str = "candidate:1 1 udp 1 203.0.113.9 9 typ relay";

fn unpacked(text: &str) -> serde_json::Value {
    let units = lz_str::decompress_from_base64(text).unwrap();
    serde_json::from_str(&String::from_utf16(&units).unwrap()).unwrap()
}

pub(super) fn cid() -> CallId {
    CallId(vec![9; 16])
}

pub(super) fn ctype(media: CallMedia, encryption: bool) -> CallType {
    CallType {
        media,
        capabilities: CallCapabilities { encryption },
    }
}

pub(super) fn voice(
    t: &mut T,
    conn: &ConnId,
    key: Option<&KeyPair>,
    range: Option<VersionRange>,
) -> Vec<Event> {
    let invitation = CallInvitation {
        call_type: ctype(CallMedia::Audio, key.is_some()),
        call_dh_pub_key: key.map(|k| public_b64(&k.public)),
        call_v_range: range,
    };
    says(
        t,
        conn,
        ChatEvent::CallInv {
            call_id: cid(),
            invitation,
        },
    )
}

pub(super) fn says(t: &mut T, conn: &ConnId, event: ChatEvent) -> Vec<Event> {
    let n = i64::try_from(t.acks().len()).unwrap() + 1;
    t.peer_says(conn, n, event)
}

pub(super) fn session(sdp: &str) -> WebRtcSession {
    WebRtcSession {
        rtc_session: sdp.into(),
        rtc_ice_candidates: format!("[\"{CAND}\"]"),
    }
}

/// Every call event the engine sent, oldest first.
pub(super) fn sent(t: &T) -> Vec<ChatEvent> {
    t.e.agent
        .sent()
        .iter()
        .flat_map(|(_, b)| parse_chat_messages(b).unwrap())
        .map(|m| m.event)
        .filter(|e| {
            matches!(
                e,
                ChatEvent::CallInv { .. }
                    | ChatEvent::CallOffer { .. }
                    | ChatEvent::CallAnswer { .. }
                    | ChatEvent::CallExtra { .. }
                    | ChatEvent::CallEnd { .. }
            )
        })
        .collect()
}

pub(super) fn call(t: &mut T, r: CallRequest) -> Vec<Event> {
    t.req(Request::Call(r))
}

pub(super) fn signals(out: &[Event]) -> Vec<CallSignal> {
    out.iter()
        .filter_map(|e| {
            if let Event::Call(s) = e {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect()
}

fn line(out: &[Event]) -> ChatMessage {
    messages(out).pop().expect("a line")
}

fn record(m: &ChatMessage) -> CallRecord {
    let MessageBody::Call(r) = m.body else {
        panic!("{:?}", m.body)
    };
    r
}

fn unread_of(t: &T, friend: i64) -> u32 {
    t.e.state.friends[&friend].unread
}

/// The caller's own invitation: the call id and key the engine sent.
fn invitation_sent(t: &T) -> (CallId, CallInvitation) {
    sent(t)
        .into_iter()
        .find_map(|e| match e {
            ChatEvent::CallInv {
                call_id,
                invitation,
            } => Some((call_id, invitation)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_voice_call_invitation_without_the_apps_encryption_is_usable() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = voice(&mut t, &conn, None, None);
    assert_eq!(
        signals(&out),
        [CallSignal::Invited {
            friend: bob,
            usable: true,
            key: None
        }]
    );
}

#[test]
fn an_official_apps_invitation_is_usable_and_brings_the_engines_frame_key() {
    for (range, version) in [(Some(SUPPORTED_CALL_VRANGE), 2), (None, 1)] {
        let mut t = T::started();
        let (bob, conn) = t.friend("bob");
        let peer = KeyPair::generate(Alg::X25519).unwrap();
        let out = voice(&mut t, &conn, Some(&peer), range);
        let [
            CallSignal::Invited {
                friend,
                usable: true,
                key: Some(key),
            },
        ] = signals(&out)[..]
        else {
            panic!("{out:?}")
        };
        assert_eq!(friend, bob);
        // The offer carries the callee's public key and the negotiated version; the peer
        // derives the same key from it.
        call(
            &mut t,
            CallRequest::Offer {
                friend: bob,
                sdp: SDP.into(),
            },
        );
        let ChatEvent::CallOffer { call_id, offer } = sent(&t).pop().unwrap() else {
            panic!()
        };
        assert_eq!(offer.call_version, Some(version));
        let ours = crate::crypto::PublicKey::from_der(
            &crate::chat::ids::b64_decode(&offer.call_dh_pub_key.unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            media_key(version, &call_id.0, &ours, &peer.private),
            Some(key)
        );
    }
}

#[test]
fn an_incompatible_call_version_is_not_reported() {
    let mut t = T::started();
    let (_, conn) = t.friend("bob");
    let out = voice(&mut t, &conn, None, Some(VersionRange::new(5, 6)));
    assert!(signals(&out).is_empty());
    assert!(t.e.state.calls.calls.is_empty());
}

#[test]
fn offers_answers_extra_candidates_and_the_end_become_signals() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    call(&mut t, CallRequest::Invite(bob));
    let (id, inv) = invitation_sent(&t);
    let peer = KeyPair::generate(Alg::X25519).unwrap();
    let offer = CallOffer {
        call_type: ctype(CallMedia::Audio, true),
        rtc_session: session(SDP),
        call_dh_pub_key: Some(public_b64(&peer.public)),
        call_version: Some(2),
    };
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallOffer {
            call_id: id.clone(),
            offer,
        },
    );
    // The caller's key: the peer's public key with our private one, which the peer mirrors.
    let our = crate::crypto::PublicKey::from_der(
        &crate::chat::ids::b64_decode(&inv.call_dh_pub_key.unwrap()).unwrap(),
    )
    .unwrap();
    let key = media_key(2, &id.0, &our, &peer.private);
    assert!(key.is_some());
    assert_eq!(
        signals(&out),
        [CallSignal::Offer {
            friend: bob,
            sdp: SDP.into(),
            candidates: vec![CAND.into()],
            key
        }]
    );
    call(
        &mut t,
        CallRequest::Answer {
            friend: bob,
            sdp: SDP.into(),
        },
    );
    let extra = CallExtraInfo {
        rtc_extra_info: WebRtcExtraInfo {
            rtc_session: None,
            rtc_ice_candidates: format!("[\"{CAND}\"]"),
        },
    };
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallExtra {
            call_id: id.clone(),
            extra,
        },
    );
    assert_eq!(
        signals(&out),
        [CallSignal::Extra {
            friend: bob,
            candidates: vec![CAND.into()]
        }]
    );
    let out = says(&mut t, &conn, ChatEvent::CallEnd { call_id: id });
    assert_eq!(signals(&out), [CallSignal::Ended(bob)]);
    assert!(t.e.state.calls.calls.is_empty());
    // The call ends before its line shows, as with the old engine.
    let ended = out.iter().position(|e| matches!(e, Event::Call(_)));
    let line = out.iter().position(|e| matches!(e, Event::Messages(_)));
    assert!(ended < line, "{out:?}");
}

#[test]
fn an_answer_with_a_description_that_cannot_be_read_is_unusable() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    voice(&mut t, &conn, None, None);
    call(
        &mut t,
        CallRequest::Offer {
            friend: bob,
            sdp: SDP.into(),
        },
    );
    let answer = CallAnswer {
        rtc_session: session("nonsense"),
    };
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallAnswer {
            call_id: cid(),
            answer,
        },
    );
    assert_eq!(signals(&out), [CallSignal::Unusable(bob)]);
}

#[test]
fn an_offer_whose_description_cannot_be_read_is_unusable() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    call(&mut t, CallRequest::Invite(bob));
    let (id, _) = invitation_sent(&t);
    let offer = CallOffer {
        call_type: ctype(CallMedia::Audio, false),
        rtc_session: session("N4IgLgFg...not lz-string"),
        call_dh_pub_key: None,
        call_version: None,
    };
    let out = says(&mut t, &conn, ChatEvent::CallOffer { call_id: id, offer });
    assert_eq!(signals(&out), [CallSignal::Unusable(bob)]);
}

#[test]
fn call_requests_send_the_apps_call_events() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    call(&mut t, CallRequest::Invite(bob));
    let (id, inv) = invitation_sent(&t);
    assert_eq!(id.0.len(), 16);
    assert_eq!(inv.call_type, ctype(CallMedia::Audio, true));
    assert_eq!(inv.call_v_range, Some(SUPPORTED_CALL_VRANGE));
    assert_eq!(inv.call_dh_pub_key.as_deref().map(str::len), Some(60));

    let offer = CallOffer {
        call_type: ctype(CallMedia::Audio, true),
        rtc_session: session(SDP),
        call_dh_pub_key: Some(public_b64(&KeyPair::generate(Alg::X25519).unwrap().public)),
        call_version: Some(2),
    };
    says(
        &mut t,
        &conn,
        ChatEvent::CallOffer {
            call_id: id.clone(),
            offer,
        },
    );
    call(
        &mut t,
        CallRequest::Answer {
            friend: bob,
            sdp: SDP.into(),
        },
    );
    let ChatEvent::CallAnswer { call_id, answer } = sent(&t).pop().unwrap() else {
        panic!()
    };
    assert_eq!(call_id, id);
    assert_eq!(unpacked(&answer.rtc_session.rtc_session)["type"], "answer");
    call(
        &mut t,
        CallRequest::Extra {
            friend: bob,
            candidates: vec!["candidate:x".into()],
        },
    );
    let ChatEvent::CallExtra { extra, .. } = sent(&t).pop().unwrap() else {
        panic!()
    };
    assert!(
        unpacked(&extra.rtc_extra_info.rtc_ice_candidates)
            .to_string()
            .contains("candidate:x")
    );
    call(&mut t, CallRequest::Connected(bob));
    assert!(t.e.state.calls.calls[&bob].connected.is_some());
    call(&mut t, CallRequest::End(bob));
    assert_eq!(sent(&t).pop(), Some(ChatEvent::CallEnd { call_id: id }));
}

#[test]
fn a_callee_offers_with_its_own_key_only_when_the_offer_asks_for_encryption() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let peer = KeyPair::generate(Alg::X25519).unwrap();
    voice(&mut t, &conn, Some(&peer), Some(SUPPORTED_CALL_VRANGE));
    call(
        &mut t,
        CallRequest::Offer {
            friend: bob,
            sdp: SDP.into(),
        },
    );
    let ChatEvent::CallOffer { offer, .. } = sent(&t).pop().unwrap() else {
        panic!()
    };
    assert_eq!(offer.call_type, ctype(CallMedia::Audio, true));
    assert!(offer.call_dh_pub_key.is_some());
    assert_eq!(unpacked(&offer.rtc_session.rtc_session)["type"], "offer");
    assert!(
        unpacked(&offer.rtc_session.rtc_ice_candidates)
            .to_string()
            .contains("candidate:1 1 udp")
    );
}

#[test]
fn a_refused_invitation_is_reported_and_a_failed_hang_up_is_not() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let out = call(&mut t, CallRequest::Invite(99));
    assert!(matches!(
        signals(&out)[..],
        [CallSignal::Refused { friend: 99, .. }]
    ));
    t.e.agent.fail_next = Some(crate::engine::agent_port::AgentFailure::Other(
        "down".into(),
    ));
    let out = call(&mut t, CallRequest::Invite(bob));
    assert!(matches!(signals(&out)[..], [CallSignal::Refused { .. }]));
    assert!(
        t.e.state.calls.calls.is_empty(),
        "a refused call is not kept"
    );
    let out = call(&mut t, CallRequest::End(bob));
    assert!(out.is_empty());
    let out = call(
        &mut t,
        CallRequest::Offer {
            friend: bob,
            sdp: SDP.into(),
        },
    );
    assert!(
        matches!(signals(&out)[..], [CallSignal::Refused { .. }]),
        "no invitation to answer"
    );
}

#[test]
fn a_blocked_friends_call_is_ended_at_once_and_never_rings() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    t.req(Request::SetBlocked {
        friend: bob,
        on: true,
    });
    let out = voice(&mut t, &conn, None, None);
    assert!(signals(&out).is_empty());
    assert_eq!(sent(&t), [ChatEvent::CallEnd { call_id: cid() }]);
    assert!(t.e.state.calls.calls.is_empty());
    assert!(messages(&out).is_empty());
}

#[test]
fn a_call_line_is_hidden_while_it_rings_and_shown_once_when_it_is_over() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = voice(&mut t, &conn, None, None);
    assert!(messages(&out).is_empty());
    let out = says(&mut t, &conn, ChatEvent::CallEnd { call_id: cid() });
    let m = line(&out);
    assert_eq!(
        record(&m),
        CallRecord {
            kind: CallKind::Missed,
            minutes: 0
        }
    );
    assert_eq!(m.author, MessageAuthor::Friend);
    assert_eq!(m.chat, ChatRef::Direct(bob));
    assert!(m.unread, "a missed call is unread");
    assert_eq!(unread_of(&t, bob), 1);
    let again = says(&mut t, &conn, ChatEvent::CallEnd { call_id: cid() });
    assert!(messages(&again).is_empty(), "reported once");
}

#[test]
fn ended_declined_and_missed_calls_read_as_their_lines() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    // Answered and connected, 20 seconds: a minute.
    voice(&mut t, &conn, None, None);
    call(
        &mut t,
        CallRequest::Offer {
            friend: bob,
            sdp: SDP.into(),
        },
    );
    call(&mut t, CallRequest::Connected(bob));
    t.clock.advance(Duration::from_secs(20));
    let m = line(&call(&mut t, CallRequest::End(bob)));
    assert_eq!(
        record(&m),
        CallRecord {
            kind: CallKind::Ended,
            minutes: 1
        }
    );
    assert!(!m.unread, "a call the user took is read");
    // Rejected: declined, read, and nothing is sent.
    voice(&mut t, &conn, None, None);
    let before = sent(&t).len();
    let m = line(&call(&mut t, CallRequest::Reject(bob)));
    assert_eq!(record(&m).kind, CallKind::Declined);
    assert!(!m.unread, "a declined call is not unread");
    assert_eq!(sent(&t).len(), before, "reject sends nothing");
    // The user's own call hung up while it rings: missed.
    call(&mut t, CallRequest::Invite(bob));
    let m = line(&call(&mut t, CallRequest::End(bob)));
    assert_eq!(
        (record(&m).kind, m.author),
        (CallKind::Missed, MessageAuthor::Me)
    );
    assert_eq!(unread_of(&t, bob), 0);
}

#[test]
fn declining_ends_the_invitation_and_the_line_reads_declined_and_read() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    voice(&mut t, &conn, None, None);
    let out = call(&mut t, CallRequest::Decline(bob));
    assert_eq!(
        sent(&t),
        [ChatEvent::CallEnd { call_id: cid() }],
        "the caller is told at once"
    );
    let m = line(&out);
    assert_eq!(record(&m).kind, CallKind::Declined);
    assert!(!m.unread);
    assert_eq!(unread_of(&t, bob), 0);
}

#[test]
fn the_friend_ending_a_call_that_still_rings_is_a_decline_for_the_caller() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    call(&mut t, CallRequest::Invite(bob));
    let (id, _) = invitation_sent(&t);
    let out = says(&mut t, &conn, ChatEvent::CallEnd { call_id: id });
    assert_eq!(signals(&out), [CallSignal::Declined(bob)]);
    let m = line(&out);
    assert_eq!(
        record(&m),
        CallRecord {
            kind: CallKind::Declined,
            minutes: 0
        }
    );
    assert_eq!(m.author, MessageAuthor::Me);
    assert_eq!(unread_of(&t, bob), 0, "the user's own line needs no read");
    // Once answered, the friend hanging up is the end of a call, not a decline.
    call(&mut t, CallRequest::Invite(bob));
    let (id, _) = sent(&t)
        .into_iter()
        .rev()
        .find_map(|e| match e {
            ChatEvent::CallInv {
                call_id,
                invitation,
            } => Some((call_id, invitation)),
            _ => None,
        })
        .unwrap();
    let offer = CallOffer {
        call_type: ctype(CallMedia::Audio, false),
        rtc_session: session(SDP),
        call_dh_pub_key: None,
        call_version: None,
    };
    says(
        &mut t,
        &conn,
        ChatEvent::CallOffer {
            call_id: id.clone(),
            offer,
        },
    );
    let out = says(&mut t, &conn, ChatEvent::CallEnd { call_id: id });
    assert_eq!(signals(&out), [CallSignal::Ended(bob)]);
}

#[test]
fn history_shows_finished_calls() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    voice(&mut t, &conn, None, None);
    call(&mut t, CallRequest::Reject(bob));
    voice(&mut t, &conn, None, None);
    says(&mut t, &conn, ChatEvent::CallEnd { call_id: cid() });
    let out = t.req(Request::History {
        chat: ChatRef::Direct(bob),
        before: None,
        count: 20,
        tag: 9,
    });
    let [Event::History { messages, .. }] = &out[..] else {
        panic!("{out:?}")
    };
    let kinds: Vec<CallKind> = messages
        .iter()
        .filter_map(|m| {
            if let MessageBody::Call(r) = &m.body {
                Some(r.kind)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(kinds, [CallKind::Declined, CallKind::Missed]);
}

#[test]
fn a_new_invitation_replaces_the_current_call_and_closes_its_line() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    voice(&mut t, &conn, None, None);
    let invitation = CallInvitation {
        call_type: ctype(CallMedia::Audio, false),
        call_dh_pub_key: None,
        call_v_range: None,
    };
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallInv {
            call_id: CallId(vec![5; 16]),
            invitation,
        },
    );
    assert_eq!(record(&line(&out)).kind, CallKind::Missed, "the old one");
    assert!(matches!(signals(&out)[..], [CallSignal::Invited { .. }]));
    assert_eq!(t.e.state.calls.calls.len(), 1, "one call per friend");
    assert_eq!(t.e.state.calls.calls[&bob].id, vec![5; 16]);
}

#[test]
fn an_event_for_another_call_or_in_the_wrong_state_is_ignored() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = says(&mut t, &conn, ChatEvent::CallEnd { call_id: cid() });
    assert!(out.is_empty(), "no current call");
    voice(&mut t, &conn, None, None);
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallEnd {
            call_id: CallId(vec![1; 16]),
        },
    );
    assert!(out.is_empty(), "wrong call id");
    let answer = CallAnswer {
        rtc_session: session(SDP),
    };
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallAnswer {
            call_id: cid(),
            answer,
        },
    );
    assert!(
        signals(&out).is_empty(),
        "an answer to an invitation we received"
    );
    assert!(t.e.state.calls.calls.contains_key(&bob));
}

#[test]
fn a_call_not_connected_in_three_minutes_is_let_go() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    voice(&mut t, &conn, None, None);
    assert_eq!(
        t.e.sleep_for(),
        Duration::from_secs(5),
        "idle: the calls' deadline is later"
    );
    t.clock.advance(Duration::from_secs(176));
    assert_eq!(
        t.e.sleep_for(),
        Duration::from_secs(4),
        "wakes for the deadline"
    );
    t.clock.advance(Duration::from_secs(4));
    t.e.tick();
    let out = t.out();
    assert_eq!(signals(&out), [CallSignal::Ended(bob)]);
    assert_eq!(record(&line(&out)).kind, CallKind::Missed);
    assert!(t.e.state.calls.calls.is_empty());
    assert!(sent(&t).is_empty(), "their invitation: nobody to tell");
}

#[test]
fn a_ringing_call_the_user_placed_tells_the_friend_when_it_is_let_go() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    call(&mut t, CallRequest::Invite(bob));
    t.clock.advance(Duration::from_secs(181));
    t.e.tick();
    let out = t.out();
    assert_eq!(record(&line(&out)).kind, CallKind::Missed);
    assert!(matches!(sent(&t).last(), Some(ChatEvent::CallEnd { .. })));
    let ended = out.iter().position(|e| matches!(e, Event::Call(_)));
    let line = out.iter().position(|e| matches!(e, Event::Messages(_)));
    assert!(ended < line, "the call ends before its line shows: {out:?}");
}

#[test]
fn a_connected_call_does_not_expire() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    voice(&mut t, &conn, None, None);
    call(
        &mut t,
        CallRequest::Offer {
            friend: bob,
            sdp: SDP.into(),
        },
    );
    call(&mut t, CallRequest::Connected(bob));
    t.clock.advance(Duration::from_secs(3600));
    t.e.tick();
    assert!(t.out().is_empty());
    assert!(t.e.state.calls.calls.contains_key(&bob));
}

#[test]
fn an_official_apps_offer_reads_its_compressed_description_candidates_and_key() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    call(&mut t, CallRequest::Invite(bob));
    let (id, inv) = invitation_sent(&t);
    let packed = |v: serde_json::Value| lz_str::compress_to_base64(&v.to_string());
    let peer = KeyPair::generate(Alg::X25519).unwrap();
    let offer = CallOffer {
        call_type: ctype(CallMedia::Audio, true),
        rtc_session: WebRtcSession {
            rtc_session: packed(serde_json::json!({ "type": "offer", "sdp": SDP })),
            rtc_ice_candidates: packed(serde_json::json!([
                { "candidate": "candidate:1 1 udp 41885439 203.0.113.5 3478 typ relay", "sdpMid": "0", "sdpMLineIndex": 0 }
            ])),
        },
        call_dh_pub_key: Some(public_b64(&peer.public)),
        call_version: Some(2),
    };
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallOffer {
            call_id: id.clone(),
            offer,
        },
    );
    let our = crate::crypto::PublicKey::from_der(
        &crate::chat::ids::b64_decode(&inv.call_dh_pub_key.unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        signals(&out),
        [CallSignal::Offer {
            friend: bob,
            sdp: SDP.into(),
            candidates: vec!["candidate:1 1 udp 41885439 203.0.113.5 3478 typ relay".into()],
            key: media_key(2, &id.0, &our, &peer.private),
        }]
    );
}

#[test]
fn calls_follow_the_friends_calls_preference_both_ways() {
    use crate::chat::preferences::FeatureAllowed;
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    t.peer_says(
        &conn,
        1,
        super::profile::their_info(None, |p| p.calls = super::profile::pref(FeatureAllowed::No)),
    );
    let out = voice(&mut t, &conn, None, None);
    assert!(signals(&out).is_empty(), "their invitation is not reported");
    let sends = t.e.agent.sent().len();
    let out = t.req(Request::Call(CallRequest::Invite(bob)));
    assert!(matches!(signals(&out)[..], [CallSignal::Refused { .. }]));
    assert_eq!(t.e.agent.sent().len(), sends, "nothing reaches them");
}
