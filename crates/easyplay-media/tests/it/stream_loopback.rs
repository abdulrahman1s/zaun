//! A game stream over loopback (ADR-0036): one side shares the test picture, the other takes
//! the stream, watches, and decodes frames that show it; the descriptions are swapped by
//! hand, as the chat engine would.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use easyplay_core::chat::media::pattern::shows_test_pattern;
use easyplay_core::chat::media::{
    FrameSlot, ShareConfig, StreamEvent, StreamFactory, StreamMedia, StreamSink, WatchConfig,
};
use easyplay_media::{PatternCapture, RtcMedia, ToneAudio};

pub struct Stream {
    pub media: Box<dyn StreamMedia>,
    pub events: Receiver<StreamEvent>,
}

fn sink() -> (StreamSink, Receiver<StreamEvent>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    (
        Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        }),
        rx,
    )
}

pub fn wait<T>(s: &Stream, what: &str, mut f: impl FnMut(&StreamEvent) -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match s.events.recv_timeout(left) {
            Ok(StreamEvent::Failed { code, detail }) => panic!("{what}: failed {code:?}: {detail}"),
            Ok(e) => {
                if let Some(t) = f(&e) {
                    return t;
                }
            }
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

pub fn description(s: &Stream) -> String {
    wait(s, "a description", |e| match e {
        StreamEvent::Description(d) => Some(d.clone()),
        _ => None,
    })
}

pub fn share(f: &RtcMedia, ice: Vec<easyplay_core::model::IceServer>, relay_only: bool) -> Stream {
    share_at(f, ice, relay_only, Some(720), 30)
}

/// A share of at most `max_height` lines and `fps` frames a second, the GPU tried first.
pub fn share_at(
    f: &RtcMedia,
    ice: Vec<easyplay_core::model::IceServer>,
    relay_only: bool,
    max_height: Option<u32>,
    fps: u32,
) -> Stream {
    let (sink, events) = sink();
    let config = ShareConfig {
        title: "Lantern Keep".into(),
        game: "g1".into(),
        pids: Vec::new(),
        max_height,
        fps,
        hardware: true,
        sound: false,
        ice,
        relay_only,
    };
    let media = f.share(config, sink).unwrap();
    Stream { media, events }
}

pub fn watch(
    f: &RtcMedia,
    frames: &FrameSlot,
    ice: Vec<easyplay_core::model::IceServer>,
    relay_only: bool,
) -> Stream {
    let (sink, events) = sink();
    let config = WatchConfig {
        ice,
        relay_only,
        frames: frames.clone(),
    };
    let media = f.watch(config, sink).unwrap();
    Stream { media, events }
}

/// Watch until a frame that shows the test picture arrives; the frames seen.
pub fn see_the_picture(frames: &FrameSlot) -> u64 {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        if let Some((n, f)) = frames.newer_than(0)
            && shows_test_pattern(&f)
        {
            std::thread::sleep(Duration::from_secs(1));
            let more = frames.number();
            assert!(more > n, "frames keep coming ({n} → {more})");
            return more;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "no picture arrived ({} frames, last {:?})",
        frames.number(),
        frames.newer_than(0).map(|(_, f)| f)
    )
}

pub fn pattern_media() -> RtcMedia {
    RtcMedia::new(Arc::new(ToneAudio::new(440.0)))
        .with_host_ips(vec!["127.0.0.1".parse().unwrap()])
        .with_capture(Arc::new(PatternCapture {
            width: 1920,
            height: 1080,
            fps: 60,
        }))
}

/// Wait until frames of `width × height` that show the test picture arrive.
pub fn see_size(frames: &FrameSlot, width: u32, height: u32) {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        if let Some((_, f)) = frames.newer_than(0)
            && (f.width, f.height) == (width, height)
            && shows_test_pattern(&f)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let last = frames.newer_than(0).map(|(_, f)| (f.width, f.height));
    panic!("no {width}×{height} picture arrived (last {last:?})");
}

/// What the share says it sends, the next time it says so.
pub fn sending(s: &Stream) -> easyplay_core::model::StreamInfo {
    wait(s, "what is sent", |e| match e {
        StreamEvent::Sending(i) => Some(*i),
        _ => None,
    })
}

#[test]
fn a_shared_test_picture_arrives_decoded_once_the_friend_watches() {
    let (fa, fb) = (pattern_media(), pattern_media());
    let a = share(&fa, Vec::new(), false);
    wait(&a, "the window", |e| {
        (*e == StreamEvent::Ready).then_some(())
    });
    let frames = FrameSlot::new();
    let b = watch(&fb, &frames, Vec::new(), false);
    let offer = description(&b);
    assert!(
        offer.contains("H264") && offer.contains("webrtc-datachannel"),
        "{offer}"
    );
    a.media.offer(offer);
    b.media.answer(description(&a));
    wait(&a, "connected", |e| {
        (*e == StreamEvent::Connected).then_some(())
    });
    wait(&b, "connected", |e| {
        (*e == StreamEvent::Connected).then_some(())
    });
    let title = wait(&b, "the title", |e| match e {
        StreamEvent::Title(t) => Some(t.clone()),
        _ => None,
    });
    assert_eq!(title, "Lantern Keep");
    // Not watched: nothing is sent.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(frames.number(), 0);
    b.media.watch(true);
    wait(&a, "the friend watching", |e| {
        (*e == StreamEvent::Watching(true)).then_some(())
    });
    let n = see_the_picture(&frames);
    eprintln!("{n} frames decoded");
    b.media.watch(false);
    wait(&a, "the friend no longer watching", |e| {
        (*e == StreamEvent::Watching(false)).then_some(())
    });
}

#[test]
fn a_new_picture_size_and_rate_while_sharing_reaches_the_watcher() {
    let (fa, fb) = (pattern_media(), pattern_media());
    let a = share_at(&fa, Vec::new(), false, Some(720), 30);
    wait(&a, "the window", |e| {
        (*e == StreamEvent::Ready).then_some(())
    });
    let frames = FrameSlot::new();
    let b = watch(&fb, &frames, Vec::new(), false);
    a.media.offer(description(&b));
    b.media.answer(description(&a));
    wait(&b, "connected", |e| {
        (*e == StreamEvent::Connected).then_some(())
    });
    b.media.watch(true);
    see_size(&frames, 1280, 720);
    let first = sending(&a);
    assert_eq!((first.width, first.height), (1280, 720));
    assert!(first.fps <= 30, "{first:?}");
    // While sharing, 1080p60: a keyframe of the new size reaches the watcher.
    a.media.set_quality(Some(1080), 60);
    see_size(&frames, 1920, 1080);
    let now = wait(&a, "the new size", |e| match e {
        StreamEvent::Sending(i) if i.height == 1080 => Some(*i),
        _ => None,
    });
    assert_eq!(now.width, 1920);
    eprintln!("sent {first:?} then {now:?}");
}
