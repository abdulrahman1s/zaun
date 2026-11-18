//! A voice call end to end (ADR-0035): two real chat engines (`zaun-chat` + libsimplex)
//! signal the call through a local SMP relay, and their call media connect and carry a tone
//! each way; then the call's line reads "Call ended" on both sides.
//!
//! Ignored: it needs the engine and a relay, run in a network namespace with only loopback
//! (a local recipe kept by the maintainer):
//!
//! ```text
//! ZAUN_SIMPLEX_LIB=/…/libsimplex.so ZAUN_SIMPLEX_LIBRARY_PATH=/…/gmp/lib:/…/zlib/lib \
//! ZAUN_TEST_SMP='smp://<fingerprint>@127.0.0.1:5223' \
//! ZAUN_TEST_TURN='turn:u:p@127.0.0.1:3478' \   # optional: relay-only media
//! cargo test -p zaun-media --test it call_e2e -- --ignored
//! ```

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zaun_core::chat::media::{CallRequest, CallSignal, Negotiation};
use zaun_core::chat::{
    ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, Request, SimplexFactory,
};
use zaun_core::model::{CallKind, MessageAuthor, MessageBody};
use zaun_media::{RtcMedia, ToneAudio, tone_strength};

use super::common::{Side, connected, description, start};
use super::relay::{factory as relay_factory, test_relays};

const WAIT: Duration = Duration::from_secs(60);

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set (see the file's header)"))
}

pub(super) fn engines(log: PathBuf) -> SimplexFactory {
    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/zaun-chat");
    assert!(helper.is_file(), "build it first: cargo build -p zaun-chat");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let bwrap = std::env::split_paths(&path)
        .map(|d| d.join("bwrap"))
        .find(|b| b.is_file());
    SimplexFactory {
        helper,
        library: env("ZAUN_SIMPLEX_LIB").into(),
        bwrap,
        library_path: std::env::var_os("ZAUN_SIMPLEX_LIBRARY_PATH"),
        log,
    }
}

pub(super) struct Peer {
    pub(super) engine: Box<dyn Engine>,
    events: Receiver<ChatEvent>,
}

impl Peer {
    pub(super) fn start(f: &SimplexFactory, dir: PathBuf, name: &str) -> Peer {
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let sink: EventSink = Arc::new(move |e: ChatEvent| {
            let _ = tx.lock().unwrap().send(e);
        });
        let config = EngineConfig {
            dir,
            key: format!("{name}-key"),
            name: name.into(),
            relays: vec![env("ZAUN_TEST_SMP")],
        };
        let p = Peer {
            engine: f.start(config, sink).expect("engine starts"),
            events: rx,
        };
        p.wait("ready", |e| matches!(e, ChatEvent::Ready(_)).then_some(()));
        p
    }

    pub(super) fn wait<T>(&self, what: &str, mut f: impl FnMut(&ChatEvent) -> Option<T>) -> T {
        let end = std::time::Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(std::time::Instant::now());
            match self.events.recv_timeout(left) {
                Ok(ChatEvent::Failed {
                    request,
                    code,
                    detail,
                }) => {
                    eprintln!("(failed {request:?} {code:?}: {detail})");
                }
                Ok(e) => {
                    if let Some(t) = f(&e) {
                        return t;
                    }
                }
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    fn signal<T>(&self, what: &str, mut f: impl FnMut(&CallSignal) -> Option<T>) -> T {
        self.wait(what, |e| match e {
            ChatEvent::Call(s) => f(s),
            _ => None,
        })
    }

    pub(super) fn friend(&self) -> i64 {
        for _ in 0..30 {
            self.engine.request(Request::Refresh);
            let id = self.wait("the list", |e| match e {
                ChatEvent::Chats { friends, .. } => {
                    Some(friends.iter().find(|f| f.ready).map(|f| f.id))
                }
                _ => None,
            });
            if let Some(id) = id {
                return id;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!("no ready friend")
    }

    /// The call's line once the call is over.
    fn call_line(&self) -> (CallKind, u32, MessageAuthor) {
        self.wait("the call's line", |e| match e {
            ChatEvent::Messages(m) => m.iter().find_map(|m| match &m.body {
                MessageBody::Call(r) => Some((r.kind, r.minutes, m.author.clone())),
                _ => None,
            }),
            _ => None,
        })
    }
}

/// Media for both sides: relay-only through the test relay when one is named, else direct
/// on loopback.
fn media(audio: &ToneAudio) -> (RtcMedia, bool, Vec<zaun_core::chat::media::IceServer>) {
    match test_relays().into_iter().next() {
        Some((_, server)) => (relay_factory(audio), true, vec![server]),
        None => {
            let f = RtcMedia::new(Arc::new(audio.clone()))
                .with_host_ips(vec!["127.0.0.1".parse().unwrap()]);
            (f, false, Vec::new())
        }
    }
}

#[test]
#[ignore = "needs libsimplex and a local SMP relay (see the file's header)"]
fn two_engines_place_a_call_and_hear_each_others_tone() {
    let tmp = tempfile::tempdir().unwrap();
    let f = engines(tmp.path().join("engine.log"));
    let a = Peer::start(&f, tmp.path().join("alice"), "alice");
    let b = Peer::start(&f, tmp.path().join("bob"), "bob");
    a.engine.request(Request::CreateInvite);
    let link = a.wait("an invitation", |e| match e {
        ChatEvent::Invite(l) => Some(l.clone()),
        _ => None,
    });
    b.engine.request(Request::Connect(link));
    let (a_bob, b_alice) = (a.friend(), b.friend());

    let (a_audio, b_audio) = (ToneAudio::new(440.0), ToneAudio::new(660.0));
    let (fa, relay_only, ice) = media(&a_audio);
    let (fb, _, _) = media(&b_audio);

    // Alice calls; Bob's side writes the offer; Alice's side answers it.
    a.engine.request(Request::Call(CallRequest::Invite(a_bob)));
    b.signal("the invitation", |s| {
        matches!(s, CallSignal::Invited { friend, usable: true, .. } if *friend == b_alice)
            .then_some(())
    });
    let mb: Side = start(&fb, Negotiation::Offer, relay_only, ice.clone());
    let offer = description(&mb);
    b.engine.request(Request::Call(CallRequest::Offer {
        friend: b_alice,
        sdp: offer,
    }));
    let offer = a.signal("the offer", |s| match s {
        CallSignal::Offer { sdp, .. } => Some(sdp.clone()),
        _ => None,
    });
    let ma: Side = start(&fa, Negotiation::Answer(offer), relay_only, ice);
    let answer = description(&ma);
    a.engine.request(Request::Call(CallRequest::Answer {
        friend: a_bob,
        sdp: answer,
    }));
    let answer = b.signal("the answer", |s| match s {
        CallSignal::Answer { sdp, .. } => Some(sdp.clone()),
        _ => None,
    });
    mb.media.answer(answer);
    connected(&ma);
    connected(&mb);
    a.engine
        .request(Request::Call(CallRequest::Connected(a_bob)));
    b.engine
        .request(Request::Call(CallRequest::Connected(b_alice)));

    std::thread::sleep(Duration::from_secs(3));
    let heard = b_audio.heard.samples();
    let strength = tone_strength(&heard, 440.0);
    assert!(
        strength > 0.5,
        "bob heard alice's tone at {strength:.2} ({} samples)",
        heard.len()
    );
    let strength = tone_strength(&a_audio.heard.samples(), 660.0);
    assert!(strength > 0.5, "alice heard bob's tone at {strength:.2}");

    a.engine.request(Request::Call(CallRequest::End(a_bob)));
    drop(ma);
    b.signal("the hang-up", |s| {
        matches!(s, CallSignal::Ended(f) if *f == b_alice).then_some(())
    });
    drop(mb);
    let (kind, minutes, author) = a.call_line();
    assert_eq!((kind, author), (CallKind::Ended, MessageAuthor::Me));
    assert_eq!(minutes, 1, "a call of seconds counts a minute");
    let (kind, _, author) = b.call_line();
    assert_eq!((kind, author), (CallKind::Ended, MessageAuthor::Friend));

    // A declined call ends at once for the caller, and both lines read "Declined".
    a.engine.request(Request::Call(CallRequest::Invite(a_bob)));
    b.signal("the second invitation", |s| {
        matches!(s, CallSignal::Invited { friend, .. } if *friend == b_alice).then_some(())
    });
    b.engine
        .request(Request::Call(CallRequest::Decline(b_alice)));
    a.signal("the decline", |s| {
        matches!(s, CallSignal::Declined(f) if *f == a_bob).then_some(())
    });
    let (kind, _, author) = a.call_line();
    assert_eq!((kind, author), (CallKind::Declined, MessageAuthor::Me));
    let (kind, _, author) = b.call_line();
    assert_eq!((kind, author), (CallKind::Declined, MessageAuthor::Friend));
    eprintln!("call end to end ok (relay only: {relay_only})");
}
