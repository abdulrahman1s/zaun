//! Relay-only calls through a local TURN server (ADR-0035): both sides offer only relay
//! candidates, so every packet goes through the relay, over UDP, TCP and TLS.
//!
//! Ignored: it needs a TURN server on loopback. a local recipe kept by the maintainer runs
//! coturn and this test inside `bwrap --unshare-net` (loopback only) with:
//!
//! ```text
//! EASYPLAY_TEST_TURN='turn:u:p@127.0.0.1:3478'
//! EASYPLAY_TEST_TURN_TCP='turn:u:p@127.0.0.1:3478?transport=tcp'
//! EASYPLAY_TEST_TURNS='turns:u:p@127.0.0.1:5349'  EASYPLAY_TEST_TURNS_CA=<CA certificate, DER>
//! ```

use std::sync::Arc;
use std::time::Duration;

use easyplay_core::chat::media::{IceServer, Negotiation};
use easyplay_media::{RtcMedia, ToneAudio, tone_strength};
use rustls::pki_types::CertificateDer;

use super::common::{connected, description, start};

/// The relays the environment names.
pub fn test_relays() -> Vec<(String, IceServer)> {
    [
        "EASYPLAY_TEST_TURN",
        "EASYPLAY_TEST_TURN_TCP",
        "EASYPLAY_TEST_TURNS",
    ]
    .iter()
    .filter_map(|v| {
        let uri = std::env::var(v).ok()?;
        Some((
            (*v).to_owned(),
            IceServer::parse(&uri).expect("a TURN server URI"),
        ))
    })
    .collect()
}

/// Call media trusting the test relay's certificate authority.
pub fn factory(audio: &ToneAudio) -> RtcMedia {
    let f = RtcMedia::new(Arc::new(audio.clone()));
    match std::env::var("EASYPLAY_TEST_TURNS_CA") {
        Ok(path) => {
            let der = std::fs::read(path).unwrap();
            f.with_extra_roots(&[CertificateDer::from(der)]).unwrap()
        }
        Err(_) => f,
    }
}

#[test]
#[ignore = "needs a local TURN server (see the file's header)"]
fn relay_only_calls_go_through_a_turn_server_over_udp_tcp_and_tls() {
    let relays = test_relays();
    assert!(
        !relays.is_empty(),
        "set EASYPLAY_TEST_TURN (see the file's header)"
    );
    for (name, server) in relays {
        let a_audio = ToneAudio::new(440.0);
        let b_audio = ToneAudio::new(660.0);
        let (fa, fb) = (factory(&a_audio), factory(&b_audio));
        let a = start(&fa, Negotiation::Offer, true, vec![server.clone()]);
        let offer = description(&a);
        assert!(offer.contains("typ relay"), "{name}: {offer}");
        assert!(
            !offer.contains("typ host") && !offer.contains("typ srflx"),
            "{name}: {offer}"
        );
        let b = start(&fb, Negotiation::Answer(offer), true, vec![server]);
        let answer = description(&b);
        assert!(
            answer.contains("typ relay") && !answer.contains("typ host"),
            "{name}"
        );
        a.media.answer(answer);
        connected(&a);
        connected(&b);
        std::thread::sleep(Duration::from_secs(2));
        let heard = b_audio.heard.samples();
        let strength = tone_strength(&heard, 440.0);
        assert!(
            strength > 0.5,
            "{name}: b heard the tone at {strength:.2} ({} samples)",
            heard.len()
        );
        assert!(
            tone_strength(&a_audio.heard.samples(), 660.0) > 0.5,
            "{name}: a hears b"
        );
        eprintln!("{name}: relayed call ok ({strength:.2})");
    }
}

/// The default path of every real call: the apps' public TURN server over TLS gives a relay
/// address. Contacts `turn.simplex.im:443`, so it runs only on request.
#[test]
#[ignore = "contacts the public turn.simplex.im"]
fn the_public_default_relay_gives_a_relay_address() {
    let ice = easyplay_core::chat::media::ice_servers(&[]);
    let f = RtcMedia::new(Arc::new(ToneAudio::new(440.0)));
    let a = start(&f, Negotiation::Offer, true, ice);
    let offer = description(&a);
    assert!(offer.contains("typ relay"), "{offer}");
    assert!(!offer.contains("typ host"), "relay only");
}
