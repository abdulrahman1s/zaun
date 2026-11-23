//! Streams (codec: tests_streams.rs): video calls without the apps' encryption, with no line.

use zaun_chat_wire::model::{
    CallKind, CallRequest, CallSignal, MessageBody, StreamRequest, StreamSignal,
};
use zaun_chat_wire::{Event, Request};

use super::calls::{CAND, SDP, cid, ctype, says, sent, session, signals, voice};
use super::{T, messages};
use crate::chat::ChatEvent;
use crate::chat::call::{CallAnswer, CallInvitation, CallMedia, CallOffer};
use crate::engine::agent_port::ConnId;

fn stream_req(t: &mut T, r: StreamRequest) -> Vec<Event> {
    t.req(Request::Stream(r))
}

fn stream_signals(out: &[Event]) -> Vec<StreamSignal> {
    out.iter()
        .filter_map(|e| {
            if let Event::Stream(s) = e {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect()
}

fn stream_inv(t: &mut T, conn: &ConnId) -> Vec<Event> {
    let invitation = CallInvitation {
        call_type: ctype(CallMedia::Video, false),
        call_dh_pub_key: None,
        call_v_range: Some(crate::chat::version::SUPPORTED_CALL_VRANGE),
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

/// A friend's stream reaches the user and is taken.
fn taken() -> (T, i64, ConnId) {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    let out = stream_inv(&mut t, &conn);
    assert_eq!(stream_signals(&out), [StreamSignal::Invited(bob)]);
    (t, bob, conn)
}

#[test]
fn a_video_invitation_from_zaun_is_a_stream_not_a_call() {
    let (mut t, bob, conn) = taken();
    stream_req(
        &mut t,
        StreamRequest::Offer {
            friend: bob,
            sdp: "v=0 offer".into(),
        },
    );
    let ChatEvent::CallOffer { offer, .. } = sent(&t).pop().unwrap() else {
        panic!()
    };
    assert_eq!(offer.call_type, ctype(CallMedia::Video, false));
    assert_eq!(offer.call_dh_pub_key, None);
    assert_eq!(offer.rtc_session.rtc_session, "v=0 offer", "plain SDP");
    assert_eq!(offer.rtc_session.rtc_ice_candidates, "[]");
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
    assert_eq!(
        stream_signals(&out),
        [StreamSignal::Answer {
            friend: bob,
            sdp: SDP.into(),
            candidates: vec![CAND.into()]
        }]
    );
    let out = says(&mut t, &conn, ChatEvent::CallEnd { call_id: cid() });
    assert_eq!(stream_signals(&out), [StreamSignal::Ended(bob)]);
}

#[test]
fn the_users_stream_is_offered_as_a_video_call_and_the_friends_offer_comes_back_as_a_stream() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    stream_req(&mut t, StreamRequest::Invite(bob));
    let ChatEvent::CallInv {
        call_id,
        invitation,
    } = sent(&t).pop().unwrap()
    else {
        panic!()
    };
    assert_eq!(invitation.call_type, ctype(CallMedia::Video, false));
    assert_eq!(invitation.call_dh_pub_key, None, "no key at all");
    let offer = CallOffer {
        call_type: ctype(CallMedia::Video, false),
        rtc_session: session(SDP),
        call_dh_pub_key: None,
        call_version: Some(2),
    };
    let out = says(
        &mut t,
        &conn,
        ChatEvent::CallOffer {
            call_id: call_id.clone(),
            offer,
        },
    );
    assert_eq!(
        stream_signals(&out),
        [StreamSignal::Offer {
            friend: bob,
            sdp: SDP.into(),
            candidates: vec![CAND.into()]
        }]
    );
    stream_req(&mut t, StreamRequest::End(bob));
    assert_eq!(sent(&t).pop(), Some(ChatEvent::CallEnd { call_id }));
    let before = sent(&t).len();
    let out = stream_req(&mut t, StreamRequest::End(bob));
    assert!(out.is_empty());
    assert_eq!(
        sent(&t).len(),
        before,
        "a stream already over needs no word"
    );
}

#[test]
fn a_streams_call_line_never_shows_while_it_runs_or_once_it_is_over() {
    let (mut t, bob, conn) = taken();
    stream_req(&mut t, StreamRequest::Connected(bob));
    let out = says(&mut t, &conn, ChatEvent::CallEnd { call_id: cid() });
    assert!(messages(&out).is_empty(), "{out:?}");
    let out = t.req(Request::History {
        chat: zaun_chat_wire::model::ChatRef::Direct(bob),
        before: None,
        count: 20,
        tag: 1,
    });
    let [Event::History { messages, .. }] = &out[..] else {
        panic!()
    };
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m.body, MessageBody::Call(_)))
    );
}

#[test]
fn a_voice_call_with_the_friend_takes_the_place_of_their_stream() {
    // Theirs: the new invitation replaces the stream.
    let (mut t, bob, conn) = taken();
    let out = voice(&mut t, &conn, None, None);
    assert_eq!(
        out.iter()
            .filter_map(|e| match e {
                Event::Stream(s) => Some(format!("{s:?}")),
                Event::Call(c) => Some(format!("{c:?}")),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            format!("{:?}", StreamSignal::Ended(bob)),
            format!(
                "{:?}",
                CallSignal::Invited {
                    friend: bob,
                    usable: true,
                    key: None
                }
            ),
        ]
    );
    assert!(messages(&out).is_empty(), "a stream has no line");
    // The user's: placing the call ends the stream first.
    let (mut t, bob, conn) = taken();
    let out = t.req(Request::Call(CallRequest::Invite(bob)));
    assert_eq!(stream_signals(&out), [StreamSignal::Ended(bob)]);
    assert!(matches!(sent(&t).last(), Some(ChatEvent::CallInv { .. })));
    // Later call events with the friend are the call's again.
    let id = sent(&t)
        .into_iter()
        .find_map(|e| {
            if let ChatEvent::CallInv { call_id, .. } = e {
                Some(call_id)
            } else {
                None
            }
        })
        .unwrap();
    let offer = CallOffer {
        call_type: ctype(CallMedia::Audio, false),
        rtc_session: session(SDP),
        call_dh_pub_key: None,
        call_version: None,
    };
    let out = says(&mut t, &conn, ChatEvent::CallOffer { call_id: id, offer });
    assert!(matches!(signals(&out)[..], [CallSignal::Offer { .. }]));
}

#[test]
fn a_blocked_friends_stream_is_let_go_at_once_and_never_reported() {
    let mut t = T::started();
    let (bob, conn) = t.friend("bob");
    t.req(Request::SetBlocked {
        friend: bob,
        on: true,
    });
    let out = stream_inv(&mut t, &conn);
    assert!(stream_signals(&out).is_empty(), "{out:?}");
    assert_eq!(sent(&t), [ChatEvent::CallEnd { call_id: cid() }]);
}

#[test]
fn a_refused_stream_request_is_reported_to_the_stream_flow() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let out = stream_req(
        &mut t,
        StreamRequest::Offer {
            friend: bob,
            sdp: "v=0".into(),
        },
    );
    assert!(matches!(
        stream_signals(&out)[..],
        [StreamSignal::Refused { .. }]
    ));
    let out = stream_req(&mut t, StreamRequest::Invite(77));
    assert!(matches!(
        stream_signals(&out)[..],
        [StreamSignal::Refused { friend: 77, .. }]
    ));
}

#[test]
fn a_voice_call_that_replaces_the_users_stream_still_gets_its_line() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    stream_req(&mut t, StreamRequest::Invite(bob));
    let out = t.req(Request::Call(CallRequest::Invite(bob)));
    assert_eq!(stream_signals(&out), [StreamSignal::Ended(bob)]);
    assert!(messages(&out).is_empty(), "the stream's line: none");
    let m = messages(&t.req(Request::Call(CallRequest::End(bob))))
        .pop()
        .expect("the call's line");
    let MessageBody::Call(r) = m.body else {
        panic!()
    };
    assert_eq!(r.kind, CallKind::Missed);
}

#[test]
fn a_refused_stream_leaves_no_call_behind_for_a_later_call() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    stream_req(
        &mut t,
        StreamRequest::Offer {
            friend: bob,
            sdp: "v=0".into(),
        },
    );
    assert!(t.e.state.calls.calls.is_empty());
    t.req(Request::Call(CallRequest::Invite(bob)));
    let m = messages(&t.req(Request::Call(CallRequest::End(bob))));
    assert!(matches!(m[..], [_]), "{m:?}");
}

#[test]
fn an_offer_that_is_not_plain_sdp_is_not_a_stream_after_all() {
    let (mut t, bob, conn) = taken();
    stream_req(&mut t, StreamRequest::Invite(bob));
    let offer = CallOffer {
        call_type: ctype(CallMedia::Video, false),
        rtc_session: session("N4IgLgFg"),
        call_dh_pub_key: None,
        call_version: Some(2),
    };
    let call_id = sent_id(&t);
    let out = says(&mut t, &conn, ChatEvent::CallOffer { call_id, offer });
    assert_eq!(stream_signals(&out), [StreamSignal::Ended(bob)]);
    assert!(matches!(sent(&t).last(), Some(ChatEvent::CallEnd { .. })));
}

fn sent_id(t: &T) -> crate::chat::ids::CallId {
    sent(t)
        .into_iter()
        .rev()
        .find_map(|e| {
            if let ChatEvent::CallInv { call_id, .. } = e {
                Some(call_id)
            } else {
                None
            }
        })
        .unwrap()
}
