//! A game stream end to end (ADR-0036): two real chat engines signal the stream as a video
//! call through a local SMP relay; the sharer sends the test picture (the portal cannot run
//! here), the friend's side takes the stream, watches, and decodes frames that show it, first at
//! 720p30 and then at 1080p60 after the sharer changes it mid-stream; the
//! stream ends when the sharer stops, and no call line or call signal appears on either side.
//!
//! Ignored: it needs the engine and a relay (a local recipe kept by the maintainer, see `call_e2e.rs`); with `ZAUN_TEST_TURN` the media is relay-only.

use std::time::Duration;

use zaun_core::chat::media::{FrameSlot, StreamRequest, StreamSignal};
use zaun_core::chat::{ChatEvent, Request};
use zaun_core::model::MessageBody;
use zaun_media::{PatternCapture, RtcMedia, ToneAudio};

use super::call_e2e::{Peer, engines};
use super::relay::{factory as relay_factory, test_relays};
use super::stream_loopback::{
    Stream, description, see_size, see_the_picture, sending, share, wait, watch as take,
};

impl Peer {
    fn stream_signal<T>(&self, what: &str, mut f: impl FnMut(&StreamSignal) -> Option<T>) -> T {
        self.wait(what, |e| match e {
            ChatEvent::Stream(s) => f(s),
            ChatEvent::Call(c) => panic!("a stream reached the call flow: {c:?}"),
            ChatEvent::Messages(m) if m.iter().any(|m| matches!(m.body, MessageBody::Call(_))) => {
                panic!("a stream showed a call line: {m:?}")
            }
            _ => None,
        })
    }
}

fn media() -> (RtcMedia, bool, Vec<zaun_core::model::IceServer>) {
    let pattern = std::sync::Arc::new(PatternCapture {
        width: 1920,
        height: 1080,
        fps: 60,
    });
    let audio = ToneAudio::new(440.0);
    match test_relays().into_iter().next() {
        Some((_, server)) => (
            relay_factory(&audio).with_capture(pattern),
            true,
            vec![server],
        ),
        None => (super::stream_loopback::pattern_media(), false, Vec::new()),
    }
}

#[test]
#[ignore = "needs a local SMP relay (see the file's header)"]
fn two_engines_carry_a_stream_and_the_friend_sees_the_test_picture() {
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
    let ((fa, relay_only, ice), (fb, _, _)) = (media(), media());

    // Alice shares: her window is "chosen", then Bob's app is offered the stream.
    let sa: Stream = share(&fa, ice.clone(), relay_only);
    wait(&sa, "the window", |e| {
        (*e == zaun_core::chat::media::StreamEvent::Ready).then_some(())
    });
    a.engine
        .request(Request::Stream(StreamRequest::Invite(a_bob)));
    b.stream_signal("the stream", |s| {
        matches!(s, StreamSignal::Invited(f) if *f == b_alice).then_some(())
    });
    // Bob's app takes it at once and writes the offer; Alice's share answers.
    let frames = FrameSlot::new();
    let sb = take(&fb, &frames, ice, relay_only);
    b.engine.request(Request::Stream(StreamRequest::Offer {
        friend: b_alice,
        sdp: description(&sb),
    }));
    let offer = a.stream_signal("the offer", |s| match s {
        StreamSignal::Offer { sdp, .. } => Some(sdp.clone()),
        _ => None,
    });
    sa.media.offer(offer);
    a.engine.request(Request::Stream(StreamRequest::Answer {
        friend: a_bob,
        sdp: description(&sa),
    }));
    let answer = b.stream_signal("the answer", |s| match s {
        StreamSignal::Answer { sdp, .. } => Some(sdp.clone()),
        _ => None,
    });
    sb.media.answer(answer);
    use zaun_core::chat::media::StreamEvent as E;
    wait(&sa, "connected", |e| (*e == E::Connected).then_some(()));
    wait(&sb, "connected", |e| (*e == E::Connected).then_some(()));
    let title = wait(&sb, "the title", |e| match e {
        E::Title(t) => Some(t.clone()),
        _ => None,
    });
    assert_eq!(title, "Lantern Keep");

    sb.media.watch(true);
    wait(&sa, "Bob watching", |e| {
        (*e == E::Watching(true)).then_some(())
    });
    let n = see_the_picture(&frames);
    see_size(&frames, 1280, 720);
    let first = sending(&sa);
    // While sharing: 1080p60, without a new portal or connection.
    sa.media.set_quality(Some(1080), 60);
    see_size(&frames, 1920, 1080);
    let then = wait(&sa, "the new size", |e| match e {
        E::Sending(i) if i.height == 1080 => Some(*i),
        _ => None,
    });
    eprintln!("sent {first:?}, then {then:?}");

    // Alice stops: Bob's side hears the stream is over.
    a.engine.request(Request::Stream(StreamRequest::End(a_bob)));
    drop(sa);
    b.stream_signal("the end", |s| {
        matches!(s, StreamSignal::Ended(f) if *f == b_alice).then_some(())
    });
    drop(sb);
    std::thread::sleep(Duration::from_secs(1));
    eprintln!("stream end to end ok ({n} frames, relay only: {relay_only})");
}
