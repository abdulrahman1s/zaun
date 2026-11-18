//! The real window capture (ADR-0036): the ScreenCast portal picks a window and its pictures
//! arrive through PipeWire.
//!
//! Ignored, built only with `--features portal`, and it refuses to run unless the session is
//! private: the user's own bus socket (`/run/user/<uid>/bus`) must be hidden and the bus,
//! Wayland and PipeWire sockets must all live in `XDG_RUNTIME_DIR`
//! (a local recipe kept by the maintainer starts dbus, PipeWire, WirePlumber, a headless sway
//! with one window, xdg-desktop-portal and its wlr backend inside `bwrap` with a tmpfs over
//! `/run/user/<uid>`). Never run it on a real desktop: it would open the portal's dialog there.

use std::path::Path;
use std::time::{Duration, Instant};

use easyplay_media::capture::portal::PortalCapture;
use easyplay_media::video::i420::I420;
use easyplay_media::{Capture, CaptureRequest, Next};

fn private_session() -> bool {
    let (Ok(rt), Ok(bus), Ok(wl)) = (
        std::env::var("XDG_RUNTIME_DIR"),
        std::env::var("DBUS_SESSION_BUS_ADDRESS"),
        std::env::var("WAYLAND_DISPLAY"),
    ) else {
        return false;
    };
    let uid = rustix::process::getuid().as_raw();
    let own_bus = format!("/run/user/{uid}/bus");
    !Path::new(&own_bus).exists()
        && rt != format!("/run/user/{uid}")
        && bus == format!("unix:path={rt}/bus")
        && Path::new(&rt).join(&wl).exists()
        && Path::new(&rt).join("pipewire-0").exists()
}

#[test]
#[ignore = "needs a private portal session (see the file's header)"]
fn the_portal_hands_over_one_window_and_its_pictures_arrive() {
    assert!(
        private_session(),
        "refusing: not a private session (see the file's header)"
    );
    let capture = PortalCapture::new();
    let req = CaptureRequest { game: "g1".into() };
    let mut source = capture.open(&req).expect("the portal gives a window");
    let mut pic = I420::default();
    let end = Instant::now() + Duration::from_secs(15);
    let mut frames = 0;
    while Instant::now() < end && frames < 10 {
        match source.next(&mut pic, Some(720), Duration::from_millis(500)) {
            Next::Frame => frames += 1,
            Next::Idle => {}
            other => panic!("the capture stopped: {other:?}"),
        }
    }
    assert!(frames > 0, "no picture arrived");
    assert!(pic.width >= 2 && pic.height >= 2 && pic.height <= 720);
    let (min, max) = pic
        .y
        .iter()
        .fold((255, 0), |(a, b), &y| (a.min(y), b.max(y)));
    eprintln!(
        "{frames} pictures, {}×{}, luma {min}..{max}",
        pic.width, pic.height
    );
    drop(source);
    // The choice is remembered for this game: no dialog the second time.
    let mut again = capture.open(&req).expect("restored without asking");
    assert!(matches!(
        again.next(&mut pic, None, Duration::from_secs(5)),
        Next::Frame | Next::Idle
    ));
}

#[test]
#[ignore = "needs a private portal session (see the file's header)"]
fn a_captured_window_reaches_a_watcher_decoded() {
    use easyplay_core::chat::media::{FrameSlot, StreamEvent};
    use easyplay_media::{RtcMedia, ToneAudio};
    use std::sync::Arc;

    use super::stream_loopback::{description, share, wait, watch};

    assert!(
        private_session(),
        "refusing: not a private session (see the file's header)"
    );
    let loopback = vec!["127.0.0.1".parse().unwrap()];
    let fa = RtcMedia::new(Arc::new(ToneAudio::new(440.0)))
        .with_host_ips(loopback.clone())
        .with_capture(Arc::new(PortalCapture::new()));
    let fb = RtcMedia::new(Arc::new(ToneAudio::new(440.0))).with_host_ips(loopback);
    let a = share(&fa, Vec::new(), false);
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
    let end = Instant::now() + Duration::from_secs(15);
    while frames.number() < 10 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let (n, f) = frames.newer_than(0).expect("the window's picture arrived");
    eprintln!("{n} frames of the window decoded, {}×{}", f.width, f.height);
    assert_eq!((f.width, f.height), (1280, 720));
}
