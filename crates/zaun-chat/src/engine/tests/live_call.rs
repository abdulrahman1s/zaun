//! A whole call's signalling against the official CLI 7.1.4, both ways: invitation, offer,
//! answer, extra candidates, end, and the frame key. The CLI passes the session strings through,
//! so this checks the transport, the key and that the LZ-String text decodes, not WebRTC itself.
//! Same servers and variables as `live_cli`.

use base64::Engine as _;
use serde_json::{Value, json};
use zaun_chat_wire::model::{CallKey, CallRequest, CallSignal};
use zaun_chat_wire::{Event, Request};

use super::Dir;
use super::live_cli::{Native, cli_event, connect, until, var};
use crate::agent::tests::cli::{Cli, kind};

const SDP: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\na=candidate:1 1 udp 2122260223 192.0.2.1 50000 typ host\r\n";
const CANDIDATE: &str = "candidate:2 1 udp 1686052607 198.51.100.7 50001 typ srflx";

fn lz(v: &Value) -> String {
    lz_str::compress_to_base64(&v.to_string())
}

fn unlz(text: &str) -> Value {
    let units = lz_str::decompress_from_base64(text).expect("LZ-String base64");
    serde_json::from_str(&String::from_utf16(&units).unwrap()).unwrap()
}

/// The CLI's `sharedKey` text as bytes: base64url, padded or not.
fn cli_key(text: &str) -> [u8; 32] {
    let b = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .expect("base64url key");
    b.try_into().expect("32 bytes")
}

fn the_session(sdp: &str, kind: &str) -> Value {
    let cands = json!([{ "candidate": CANDIDATE, "sdpMid": "0", "sdpMLineIndex": 0 }]);
    json!({
        "rtcSession": lz(&json!({ "type": kind, "sdp": sdp })),
        "rtcIceCandidates": lz(&cands),
    })
}

fn signal<T>(n: &mut Native, f: impl Fn(&CallSignal) -> Option<T>) -> Option<T> {
    let i = n
        .events
        .iter()
        .position(|e| matches!(e, Event::Call(s) if f(s).is_some()))?;
    match n.events.remove(i) {
        Event::Call(s) => f(&s),
        _ => None,
    }
}

#[test]
#[ignore = "needs a local smp-server, xftp-server and CLI; see live_cli"]
fn a_call_placed_from_here_and_one_from_the_cli_signal_end_to_end_with_the_same_key() {
    let dir = Dir::new();
    let mut n = Native::start(&dir);
    let mut c = Cli::open(var("ZAUN_TEST_CLI").parse().unwrap());
    let (cid, bob) = connect(&mut n, &mut c);

    // Alice calls: bob's invitation carries the key, bob offers, alice answers.
    n.req(Request::Call(CallRequest::Invite(bob)));
    let inv = until(&mut n, &mut c, "bob is called", |_, c| {
        cli_event(c, "callInvitation", |_| true)
    });
    let theirs = inv
        .pointer("/callInvitation/sharedKey")
        .and_then(Value::as_str)
        .map(cli_key)
        .expect("an encrypted call has a key");
    let offer = json!({
        "callType": { "media": "audio", "capabilities": { "encryption": true } },
        "rtcSession": the_session(SDP, "offer"),
    });
    let r = c.cmd(&format!("/_call offer @{cid} {offer}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    let (sdp, candidates, key) = until(&mut n, &mut c, "alice has the offer", |n, _| {
        signal(n, |s| match s {
            CallSignal::Offer {
                sdp,
                candidates,
                key,
                ..
            } => Some((sdp.clone(), candidates.clone(), *key)),
            _ => None,
        })
    });
    assert_eq!(sdp, SDP);
    assert!(candidates.iter().any(|c| c == CANDIDATE), "{candidates:?}");
    assert_eq!(
        key,
        Some(CallKey(theirs)),
        "both sides derive the same frame key"
    );

    let answer_sdp = SDP.replace("o=- 1 2", "o=- 3 4");
    n.req(Request::Call(CallRequest::Answer {
        friend: bob,
        sdp: answer_sdp.clone(),
    }));
    let answered = until(&mut n, &mut c, "bob has the answer", |_, c| {
        cli_event(c, "callAnswer", |_| true)
    });
    let session = answered
        .pointer("/answer/rtcSession")
        .and_then(Value::as_str)
        .unwrap();
    assert_eq!(
        unlz(session),
        json!({ "type": "answer", "sdp": answer_sdp }),
        "the app's LZ-String reads what the engine packed"
    );
    let extra =
        json!({ "rtcIceCandidates": lz(&json!([{ "candidate": CANDIDATE, "sdpMLineIndex": 0 }])) });
    let r = c.cmd(&format!("/_call extra @{cid} {extra}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    until(&mut n, &mut c, "alice has the extra candidate", |n, _| {
        signal(n, |s| match s {
            CallSignal::Extra { candidates, .. } if candidates.iter().any(|c| c == CANDIDATE) => {
                Some(())
            }
            _ => None,
        })
    });
    n.req(Request::Call(CallRequest::End(bob)));
    until(&mut n, &mut c, "bob's call ends", |_, c| {
        cli_event(c, "callEnded", |_| true)
    });

    // Bob calls: alice offers, bob answers, bob hangs up.
    let call = json!({"media": "video", "capabilities": {"encryption": true}});
    let r = c.cmd(&format!("/_call invite @{cid} {call}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    let key = until(&mut n, &mut c, "alice is called", |n, _| {
        signal(n, |s| match s {
            CallSignal::Invited {
                usable: true, key, ..
            } => Some(*key),
            _ => None,
        })
    });
    n.req(Request::Call(CallRequest::Offer {
        friend: bob,
        sdp: SDP.into(),
    }));
    let offered = until(&mut n, &mut c, "bob has the offer", |_, c| {
        cli_event(c, "callOffer", |_| true)
    });
    let session = offered
        .pointer("/offer/rtcSession")
        .and_then(Value::as_str)
        .unwrap();
    assert_eq!(unlz(session), json!({ "type": "offer", "sdp": SDP }));
    let shared = offered
        .pointer("/sharedKey")
        .and_then(Value::as_str)
        .map(cli_key);
    assert_eq!(key, shared.map(CallKey), "the callee's key is the caller's");
    let r = c.cmd(&format!(
        "/_call answer @{cid} {}",
        the_session(&answer_sdp, "answer")
    ));
    assert!(!kind(&r).contains("Error"), "{r}");
    until(&mut n, &mut c, "alice has the answer", |n, _| {
        signal(n, |s| match s {
            CallSignal::Answer { sdp, .. } if *sdp == answer_sdp => Some(()),
            _ => None,
        })
    });
    let r = c.cmd(&format!("/_call end @{cid}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    until(&mut n, &mut c, "alice's call ends", |n, _| {
        signal(n, |s| {
            matches!(s, CallSignal::Ended(f) if *f == bob).then_some(())
        })
    });
}
