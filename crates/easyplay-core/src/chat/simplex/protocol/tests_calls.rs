//! The call codec (ADR-0035) against JSON recorded from simplex-chat 7.0 (two `easyplay-chat`
//! helpers through a local relay, a local recipe kept by the maintainer).

use std::path::Path;

use serde_json::Value;

use super::*;
use crate::chat::media::{CallRequest, CallSignal};
use crate::model::{CallKind, CallRecord};

fn fixture(name: &str) -> Value {
    let text = match name {
        "active_user" => include_str!("../fixtures/active_user.json"),
        "chat_started" => include_str!("../fixtures/chat_started.json"),
        "call_invitation" => include_str!("../fixtures/call_invitation.json"),
        "call_invitation_audio" => include_str!("../fixtures/call_invitation_audio.json"),
        "call_offer" => include_str!("../fixtures/call_offer.json"),
        "call_answer" => include_str!("../fixtures/call_answer.json"),
        "call_extra" => include_str!("../fixtures/call_extra.json"),
        "call_ended" => include_str!("../fixtures/call_ended.json"),
        "call_end_no_call" => include_str!("../fixtures/call_end_no_call.json"),
        "call_item_pending_rcv" => include_str!("../fixtures/call_item_pending_rcv.json"),
        "call_item_ended_snd" => include_str!("../fixtures/call_item_ended_snd.json"),
        "call_item_ended_rcv" => include_str!("../fixtures/call_item_ended_rcv.json"),
        "call_item_rejected" => include_str!("../fixtures/call_item_rejected.json"),
        "call_item_missed_snd" => include_str!("../fixtures/call_item_missed_snd.json"),
        "call_item_missed_rcv" => include_str!("../fixtures/call_item_missed_rcv.json"),
        "history_calls" => include_str!("../fixtures/history_calls.json"),
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

fn cmds(out: &Output) -> Vec<(u64, String)> {
    out.lines
        .iter()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            (
                v["corr"].as_u64().unwrap(),
                v["cmd"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// Taken through start-up; the list (4), address (5) and relay check (6) stay unanswered.
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

fn call(c: &mut Codec, r: CallRequest) -> (u64, String) {
    let out = c.request(Request::Call(r));
    cmds(&out).remove(0)
}

#[test]
fn a_voice_call_invitation_without_the_apps_encryption_is_usable() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("call_invitation_audio")));
    assert_eq!(
        out.events,
        vec![ChatEvent::Call(CallSignal::Invited {
            friend: 3,
            usable: true,
            key: None,
        })]
    );
}

#[test]
fn an_official_apps_invitation_is_usable_and_brings_the_engines_frame_key() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("call_invitation")));
    let key =
        crate::chat::media::CallKey::from_base64url("WeZqqqH86JoKpu_Q92CdAzcRsnMe6kB-6rnwkg7SYC0=");
    assert!(key.is_some());
    assert_eq!(
        out.events,
        vec![ChatEvent::Call(CallSignal::Invited {
            friend: 3,
            usable: true,
            key,
        })]
    );
}

#[test]
fn offers_answers_extra_candidates_and_the_end_become_signals() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("call_offer")));
    let [
        ChatEvent::Call(CallSignal::Offer {
            friend: 3,
            sdp,
            candidates,
            key: None,
        }),
    ] = out.events.as_slice()
    else {
        panic!("{:?}", out.events)
    };
    assert!(sdp.starts_with("v=0\r\n") && candidates.is_empty());
    let out = c.on_line(&event(&fixture("call_answer")));
    assert!(matches!(
        &out.events[..],
        [ChatEvent::Call(CallSignal::Answer { friend: 3, .. })]
    ));
    let out = c.on_line(&event(&fixture("call_extra")));
    assert_eq!(
        out.events,
        vec![ChatEvent::Call(CallSignal::Extra {
            friend: 3,
            candidates: vec!["candidate:1 1 udp 2113937151 127.0.0.1 40000 typ host".into()],
        })]
    );
    let out = c.on_line(&event(&fixture("call_ended")));
    assert_eq!(out.events, vec![ChatEvent::Call(CallSignal::Ended(3))]);
}

#[test]
fn an_offer_whose_description_cannot_be_read_is_unusable() {
    let mut c = started();
    let mut offer = fixture("call_offer");
    offer["result"]["offer"]["rtcSession"] = "N4IgLgFg...not lz-string".into();
    let out = c.on_line(&event(&offer));
    assert_eq!(out.events, vec![ChatEvent::Call(CallSignal::Unusable(3))]);
}

#[test]
fn an_official_apps_offer_reads_its_compressed_description_candidates_and_key() {
    let mut c = started();
    let mut offer = fixture("call_offer");
    let sdp = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\n";
    let packed = |v: Value| lz_str::compress_to_base64(&v.to_string());
    offer["result"]["offer"]["rtcSession"] =
        packed(serde_json::json!({ "type": "offer", "sdp": sdp })).into();
    offer["result"]["offer"]["rtcIceCandidates"] = packed(serde_json::json!([
        { "candidate": "candidate:1 1 udp 41885439 203.0.113.5 3478 typ relay", "sdpMid": "0", "sdpMLineIndex": 0 }
    ]))
    .into();
    offer["result"]["sharedKey"] = "WeZqqqH86JoKpu_Q92CdAzcRsnMe6kB-6rnwkg7SYC0=".into();
    let out = c.on_line(&event(&offer));
    let [
        ChatEvent::Call(CallSignal::Offer {
            friend: 3,
            sdp: got,
            candidates,
            key: Some(_),
        }),
    ] = out.events.as_slice()
    else {
        panic!("{:?}", out.events)
    };
    assert_eq!(got, sdp);
    assert_eq!(
        candidates,
        &["candidate:1 1 udp 41885439 203.0.113.5 3478 typ relay"]
    );
}

/// The JSON a `WebRTCSession` field holds, uncompressed.
fn unpacked(v: &Value) -> Value {
    let units = lz_str::decompress_from_base64(v.as_str().unwrap_or_default()).unwrap_or_default();
    serde_json::from_str(&String::from_utf16(&units).unwrap_or_default()).unwrap_or_default()
}

#[test]
fn call_requests_render_the_engines_call_commands_in_the_apps_form() {
    let mut c = started();
    let (_, invite) = call(&mut c, CallRequest::Invite(3));
    assert_eq!(
        invite,
        r#"/_call invite @3 {"capabilities":{"encryption":true},"media":"audio"}"#
    );
    let sdp = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\na=candidate:1 1 udp 1 203.0.113.9 9 typ relay\r\n";
    let (_, offer) = call(
        &mut c,
        CallRequest::Offer {
            friend: 3,
            sdp: sdp.into(),
        },
    );
    let json: Value =
        serde_json::from_str(offer.strip_prefix("/_call offer @3 ").unwrap()).unwrap();
    assert_eq!(
        unpacked(&json["rtcSession"]["rtcSession"]),
        serde_json::json!({ "type": "offer", "sdp": sdp })
    );
    assert_eq!(
        unpacked(&json["rtcSession"]["rtcIceCandidates"]),
        serde_json::json!([{ "candidate": "candidate:1 1 udp 1 203.0.113.9 9 typ relay", "sdpMid": "0", "sdpMLineIndex": 0 }])
    );
    assert_eq!(json["callType"]["capabilities"]["encryption"], true);
    let (_, answer) = call(
        &mut c,
        CallRequest::Answer {
            friend: 3,
            sdp: sdp.into(),
        },
    );
    let json: Value =
        serde_json::from_str(answer.strip_prefix("/_call answer @3 ").unwrap()).unwrap();
    assert_eq!(unpacked(&json["rtcSession"])["type"], "answer");
    let (_, extra) = call(
        &mut c,
        CallRequest::Extra {
            friend: 3,
            candidates: vec!["candidate:x".into()],
        },
    );
    let json: Value =
        serde_json::from_str(extra.strip_prefix("/_call extra @3 ").unwrap()).unwrap();
    assert_eq!(
        unpacked(&json["rtcIceCandidates"]),
        serde_json::json!([{ "candidate": "candidate:x", "sdpMLineIndex": 0 }])
    );
    assert_eq!(
        call(&mut c, CallRequest::Connected(3)).1,
        "/_call status @3 connected"
    );
    assert_eq!(call(&mut c, CallRequest::Reject(3)).1, "/_call reject @3");
    assert_eq!(call(&mut c, CallRequest::End(3)).1, "/_call end @3");
}

#[test]
fn a_refused_invitation_is_reported_and_a_failed_hang_up_is_not() {
    let mut c = started();
    let (corr, _) = call(&mut c, CallRequest::Invite(3));
    let out = c.on_line(&resp(corr, &fixture("call_end_no_call")));
    assert!(matches!(
        &out.events[..],
        [ChatEvent::Call(CallSignal::Refused { friend: 3, .. })]
    ));
    let (corr, _) = call(&mut c, CallRequest::End(3));
    let out = c.on_line(&resp(corr, &fixture("call_end_no_call")));
    assert!(out.events.is_empty());
}

#[test]
fn a_blocked_friends_call_is_ended_at_once_and_never_rings() {
    let mut c = started();
    c.request(Request::SetBlocked {
        friend: 3,
        on: true,
    });
    let out = c.on_line(&event(&fixture("call_invitation_audio")));
    assert!(out.events.is_empty());
    assert_eq!(cmds(&out)[0].1, "/_call end @3");
}

#[test]
fn a_call_line_is_hidden_while_it_rings_and_shown_once_when_it_is_over() {
    let mut c = started();
    let out = c.on_line(&event(&fixture("call_item_pending_rcv")));
    assert!(out.events.is_empty(), "{:?}", out.events);
    let out = c.on_line(&event(&fixture("call_item_missed_rcv")));
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert_eq!(
        m[0].body,
        MessageBody::Call(CallRecord {
            kind: CallKind::Missed,
            minutes: 0
        })
    );
    assert_eq!(m[0].author, MessageAuthor::Friend);
    let again = c.on_line(&event(&fixture("call_item_missed_rcv")));
    assert!(again.events.is_empty(), "reported once");
}

#[test]
fn ended_declined_and_missed_calls_read_as_their_lines() {
    // Each from its own codec: the fixtures come from two users whose item numbers overlap.
    let kind = |f: &str| {
        let out = started().on_line(&event(&fixture(f)));
        match out.events.as_slice() {
            [ChatEvent::Messages(m)] => match &m[0].body {
                MessageBody::Call(r) => (*r, m[0].author.clone(), m[0].unread),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    };
    let (r, author, _) = kind("call_item_ended_snd");
    assert_eq!((r.kind, author), (CallKind::Ended, MessageAuthor::Me));
    assert_eq!(r.minutes, 1, "a call of seconds counts a minute");
    let (r, _, unread) = kind("call_item_rejected");
    assert_eq!(r.kind, CallKind::Declined);
    assert!(!unread, "a declined call is not unread");
    let (r, author, _) = kind("call_item_missed_snd");
    assert_eq!((r.kind, author), (CallKind::Missed, MessageAuthor::Me));
}

#[test]
fn declining_ends_the_invitation_and_the_line_reads_declined_and_read() {
    let mut c = started();
    let (_, cmd) = call(&mut c, CallRequest::Decline(3));
    assert_eq!(cmd, "/_call end @3", "the caller is told at once");
    // The engine marks the line missed (and unread); it reads as declined, and is read.
    let out = c.on_line(&event(&fixture("call_item_missed_rcv")));
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let MessageBody::Call(r) = &m[0].body else {
        panic!("{:?}", m[0].body)
    };
    assert_eq!(r.kind, CallKind::Declined);
    assert!(!m[0].unread);
    assert_eq!(cmds(&out)[0].1, format!("/_read chat items @3 {}", m[0].id));
}

#[test]
fn the_friend_ending_a_call_that_still_rings_is_a_decline_for_the_caller() {
    let mut c = started();
    call(&mut c, CallRequest::Invite(3));
    let out = c.on_line(&event(&fixture("call_ended")));
    assert_eq!(out.events, vec![ChatEvent::Call(CallSignal::Declined(3))]);
    let out = c.on_line(&event(&fixture("call_item_missed_snd")));
    let [ChatEvent::Messages(m)] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    assert_eq!(
        m[0].body,
        MessageBody::Call(CallRecord {
            kind: CallKind::Declined,
            minutes: 0
        })
    );
    assert_eq!(m[0].author, MessageAuthor::Me);
    assert!(out.lines.is_empty(), "the user's own line needs no read");
    // Once answered, the friend hanging up is the end of a call, not a decline.
    call(&mut c, CallRequest::Invite(3));
    c.on_line(&event(&fixture("call_offer")));
    let out = c.on_line(&event(&fixture("call_ended")));
    assert_eq!(out.events, vec![ChatEvent::Call(CallSignal::Ended(3))]);
}

#[test]
fn history_shows_finished_calls() {
    let mut c = started();
    let out = c.request(Request::History {
        chat: ChatRef::Direct(3),
        before: None,
        count: 20,
        tag: 9,
    });
    let corr = cmds(&out)[0].0;
    let out = c.on_line(&resp(corr, &fixture("history_calls")));
    let [ChatEvent::History { messages, .. }] = out.events.as_slice() else {
        panic!("{:?}", out.events)
    };
    let kinds: Vec<CallKind> = messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::Call(r) => Some(r.kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![CallKind::Ended, CallKind::Declined, CallKind::Missed]
    );
}
