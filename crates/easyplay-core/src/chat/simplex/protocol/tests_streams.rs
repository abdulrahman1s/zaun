//! The stream codec (ADR-0036) against JSON recorded from simplex-chat 7.0 (three
//! `easyplay-chat` helpers through a local relay, a local recipe kept by the maintainer).

use std::path::Path;

use serde_json::Value;

use super::*;
use crate::chat::media::{CallRequest, CallSignal, StreamRequest, StreamSignal};
use crate::model::{CallKind, CallRecord};

fn fixture(name: &str) -> Value {
    let text = match name {
        "active_user" => include_str!("../fixtures/active_user.json"),
        "chat_started" => include_str!("../fixtures/chat_started.json"),
        "call_invitation_audio" => include_str!("../fixtures/call_invitation_audio.json"),
        "call_end_no_call" => include_str!("../fixtures/call_end_no_call.json"),
        "stream_invitation" => include_str!("../fixtures/stream_invitation.json"),
        "stream_offer" => include_str!("../fixtures/stream_offer.json"),
        "stream_answer" => include_str!("../fixtures/stream_answer.json"),
        "stream_ended" => include_str!("../fixtures/stream_ended.json"),
        "stream_item_pending_rcv" => include_str!("../fixtures/stream_item_pending_rcv.json"),
        "stream_item_ended_rcv" => include_str!("../fixtures/stream_item_ended_rcv.json"),
        "stream_item_pending_snd" => include_str!("../fixtures/stream_item_pending_snd.json"),
        "stream_item_replaced_snd" => include_str!("../fixtures/stream_item_replaced_snd.json"),
        "call_after_stream_pending_snd" => {
            include_str!("../fixtures/call_after_stream_pending_snd.json")
        }
        "call_after_stream_missed_snd" => {
            include_str!("../fixtures/call_after_stream_missed_snd.json")
        }
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).unwrap()
}

fn resp(corr: u64, body: &Value) -> String {
    serde_json::json!({ "t": "resp", "corr": corr, "body": body }).to_string()
}

fn event(body: &Value) -> String {
    serde_json::json!({ "t": "event", "body": body }).to_string()
}

fn cmds(out: &Output) -> Vec<String> {
    out.lines
        .iter()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            v["cmd"].as_str().unwrap().to_owned()
        })
        .collect()
}

fn started() -> Codec {
    let mut c = Codec::new("bob", Vec::new(), Path::new("/d/chat"));
    c.on_line(r#"{"t":"ready"}"#);
    c.on_line(&resp(1, &fixture("active_user")));
    c.on_line(&resp(
        2,
        &serde_json::json!({ "result": { "type": "cmdOk" } }),
    ));
    c.on_line(&resp(3, &fixture("chat_started")));
    assert!(c.started());
    c
}

/// Alice's stream reaches Bob (contact 3) and is taken.
fn taken() -> Codec {
    let mut c = started();
    let out = c.on_line(&event(&fixture("stream_invitation")));
    assert_eq!(
        out.events,
        vec![ChatEvent::Stream(StreamSignal::Invited(3))]
    );
    c
}

#[test]
fn a_video_invitation_from_easyplay_is_a_stream_not_a_call() {
    let mut c = taken();
    let out = c.request(Request::Stream(StreamRequest::Offer {
        friend: 3,
        sdp: "v=0 offer".into(),
    }));
    let cmd = &cmds(&out)[0];
    assert!(cmd.starts_with("/_call offer @3 "), "{cmd}");
    assert!(cmd.contains(r#""media":"video""#), "{cmd}");
    let out = c.on_line(&event(&fixture("stream_answer")));
    assert!(matches!(
        &out.events[..],
        [ChatEvent::Stream(StreamSignal::Answer { friend: 3, .. })]
    ));
    let out = c.on_line(&event(&fixture("stream_ended")));
    assert_eq!(out.events, vec![ChatEvent::Stream(StreamSignal::Ended(3))]);
}

#[test]
fn the_users_stream_is_offered_as_a_video_call_and_the_friends_offer_comes_back_as_a_stream() {
    let mut c = started();
    let out = c.request(Request::Stream(StreamRequest::Invite(3)));
    assert_eq!(
        cmds(&out),
        vec![r#"/_call invite @3 {"capabilities":{"encryption":false},"media":"video"}"#]
    );
    let out = c.on_line(&event(&fixture("stream_offer")));
    assert!(matches!(
        &out.events[..],
        [ChatEvent::Stream(StreamSignal::Offer { friend: 3, sdp, .. })] if sdp.starts_with("v=0")
    ));
    let out = c.request(Request::Stream(StreamRequest::End(3)));
    assert_eq!(cmds(&out), vec!["/_call end @3"]);
    let out = c.request(Request::Stream(StreamRequest::End(3)));
    assert!(out.lines.is_empty(), "a stream already over needs no word");
}

#[test]
fn a_streams_call_item_never_shows_while_it_runs_or_once_it_is_over() {
    let mut c = taken();
    let out = c.on_line(&event(&fixture("stream_item_pending_rcv")));
    assert!(out.events.is_empty(), "{:?}", out.events);
    c.on_line(&event(&fixture("stream_ended")));
    let out = c.on_line(&event(&fixture("stream_item_ended_rcv")));
    assert!(out.events.is_empty(), "{:?}", out.events);
}

#[test]
fn a_voice_call_with_the_friend_takes_the_place_of_their_stream() {
    // Theirs: the engine replaces the stream with the new invitation.
    let mut c = taken();
    let out = c.on_line(&event(&fixture("call_invitation_audio")));
    assert_eq!(
        out.events,
        vec![
            ChatEvent::Stream(StreamSignal::Ended(3)),
            ChatEvent::Call(CallSignal::Invited {
                friend: 3,
                usable: true,
                key: None,
            }),
        ]
    );
    // The user's: placing the call ends the stream first.
    let mut c = taken();
    let out = c.request(Request::Call(CallRequest::Invite(3)));
    assert_eq!(out.events, vec![ChatEvent::Stream(StreamSignal::Ended(3))]);
    assert!(cmds(&out)[0].starts_with("/_call invite @3 "));
    // Later call events with the friend are the call's again.
    let out = c.on_line(&event(&fixture("stream_offer")));
    assert!(matches!(
        &out.events[..],
        [ChatEvent::Call(CallSignal::Offer { friend: 3, .. })]
    ));
}

#[test]
fn a_blocked_friends_stream_is_let_go_at_once_and_never_reported() {
    let mut c = started();
    c.request(Request::SetBlocked {
        friend: 3,
        on: true,
    });
    let out = c.on_line(&event(&fixture("stream_invitation")));
    assert!(out.events.is_empty(), "{:?}", out.events);
    assert_eq!(cmds(&out), vec!["/_call end @3"]);
}

#[test]
fn a_refused_stream_request_is_reported_to_the_stream_flow() {
    let mut c = started();
    let out = c.request(Request::Stream(StreamRequest::Invite(3)));
    let corr = serde_json::from_str::<Value>(&out.lines[0]).unwrap()["corr"]
        .as_u64()
        .unwrap();
    let out = c.on_line(&resp(corr, &fixture("call_end_no_call")));
    assert!(matches!(
        &out.events[..],
        [ChatEvent::Stream(StreamSignal::Refused { friend: 3, .. })]
    ));
}

#[test]
fn a_voice_call_that_replaces_the_users_stream_still_gets_its_line() {
    // Recorded on the sharer's side: the stream's item (15) ends when the voice invitation
    // replaces it; the voice call's own item (17) is then missed.
    let mut c = started();
    c.request(Request::Stream(StreamRequest::Invite(3)));
    assert!(
        c.on_line(&event(&fixture("stream_item_pending_snd")))
            .events
            .is_empty()
    );
    let out = c.request(Request::Call(CallRequest::Invite(3)));
    assert_eq!(out.events, vec![ChatEvent::Stream(StreamSignal::Ended(3))]);
    let out = c.on_line(&event(&fixture("stream_item_replaced_snd")));
    assert!(out.events.is_empty(), "the stream's line: {:?}", out.events);
    assert!(
        c.on_line(&event(&fixture("call_after_stream_pending_snd")))
            .events
            .is_empty()
    );
    let out = c.on_line(&event(&fixture("call_after_stream_missed_snd")));
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
        panic!("the call's line is missing: {:?}", out.events)
    };
    assert_eq!(
        m[0].body,
        MessageBody::Call(CallRecord {
            kind: CallKind::Missed,
            minutes: 0
        })
    );
}

#[test]
fn a_refused_stream_hides_no_later_call_line() {
    let mut c = started();
    let out = c.request(Request::Stream(StreamRequest::Invite(3)));
    let corr = serde_json::from_str::<Value>(&out.lines[0]).unwrap()["corr"]
        .as_u64()
        .unwrap();
    c.on_line(&resp(corr, &fixture("call_end_no_call")));
    c.request(Request::Call(CallRequest::Invite(3)));
    c.on_line(&event(&fixture("call_after_stream_pending_snd")));
    let out = c.on_line(&event(&fixture("call_after_stream_missed_snd")));
    assert!(
        matches!(&out.events[..], [ChatEvent::Messages(_)]),
        "{:?}",
        out.events
    );
}
