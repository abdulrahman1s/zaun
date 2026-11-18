//! Two calls' media connect over loopback with direct candidates and hear each other's tone
//! (ADR-0035): the session descriptions are swapped by hand, as the chat engine would.

use std::sync::Arc;
use std::time::Duration;

use easyplay_core::chat::media::{MediaEvent, Negotiation};
use easyplay_media::{RtcMedia, ToneAudio, tone_strength};

use super::common::{connected, description, start, wait};

#[test]
fn two_calls_on_loopback_connect_directly_and_hear_each_others_tone() {
    let a_audio = ToneAudio::new(440.0);
    let b_audio = ToneAudio::new(660.0);
    let loopback = vec!["127.0.0.1".parse().unwrap()];
    let fa = RtcMedia::new(Arc::new(a_audio.clone())).with_host_ips(loopback.clone());
    let fb = RtcMedia::new(Arc::new(b_audio.clone())).with_host_ips(loopback);

    let a = start(&fa, Negotiation::Offer, false, Vec::new());
    let offer = description(&a);
    assert!(
        offer.contains("typ host") && offer.contains("opus"),
        "{offer}"
    );
    let b = start(&fb, Negotiation::Answer(offer), false, Vec::new());
    let answer = description(&b);
    a.media.answer(answer);
    connected(&a);
    connected(&b);

    std::thread::sleep(Duration::from_secs(2));
    let heard_by_b = b_audio.heard.samples();
    let heard_by_a = a_audio.heard.samples();
    assert!(
        heard_by_b.len() > 48_000,
        "b heard {} samples",
        heard_by_b.len()
    );
    let (at_440, at_660) = (
        tone_strength(&heard_by_b, 440.0),
        tone_strength(&heard_by_b, 660.0),
    );
    assert!(
        at_440 > 0.5 && at_660 < 0.1,
        "b heard 440 Hz {at_440:.2}, 660 Hz {at_660:.2}"
    );
    assert!(tone_strength(&heard_by_a, 660.0) > 0.5, "a hears b's tone");

    // The path is measured about once a second: a round trip, and next to no loss here.
    let (rtt_ms, loss) = wait(&a, "a quality measure", |e| match e {
        MediaEvent::Quality { rtt_ms, loss } => Some((*rtt_ms, *loss)),
        _ => None,
    });
    assert!(rtt_ms < 1_000 && loss < 50, "{rtt_ms} ms, {loss} % lost");

    // Muted, a sends nothing: its own level and b's level of it fall to zero.
    a.media.set_muted(true);
    wait(&a, "a quiet level", |e| {
        matches!(e, MediaEvent::Levels { me: 0, .. }).then_some(())
    });
    wait(&b, "a quiet friend", |e| {
        matches!(e, MediaEvent::Levels { friend: 0, .. }).then_some(())
    });
}

#[test]
fn with_noise_suppression_on_the_sound_still_gets_through() {
    let a_audio = ToneAudio::new(440.0);
    let b_audio = ToneAudio::new(660.0);
    let loopback = vec!["127.0.0.1".parse().unwrap()];
    let fa = RtcMedia::new(Arc::new(a_audio)).with_host_ips(loopback.clone());
    let fb = RtcMedia::new(Arc::new(b_audio.clone())).with_host_ips(loopback);
    let a = super::common::start_with(&fa, Negotiation::Offer, true);
    let b = super::common::start_with(&fb, Negotiation::Answer(description(&a)), true);
    a.media.answer(description(&b));
    connected(&a);
    connected(&b);
    std::thread::sleep(Duration::from_secs(2));
    let heard = b_audio.heard.samples();
    assert!(heard.len() > 48_000, "b heard {} samples", heard.len());
}
