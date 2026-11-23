//! The stored form of a ratchet: complete, versioned, and resumable mid-conversation.
use super::support::{Pair, handshake, recv, send};
use zaun_chat::encoding::{Decode, Encode};
use zaun_chat::ratchet::{PrivParams, Ratchet};

#[test]
fn a_ratchet_stored_mid_conversation_with_skipped_keys_continues_identically() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(true, true);
    assert_eq!(recv(&mut alice, &send(&mut bob, "1")), "1");
    let held = send(&mut bob, "held");
    assert_eq!(recv(&mut bob, &send(&mut alice, "2")), "2");
    let held2 = send(&mut bob, "held2");
    assert_eq!(recv(&mut alice, &send(&mut bob, "3")), "3");
    assert!(alice.skipped_len() > 0);
    let (mut a2, mut b2) = (super::support::reload(&alice), super::support::reload(&bob));
    assert_eq!(a2.to_bytes().unwrap(), alice.to_bytes().unwrap());
    assert_eq!(recv(&mut a2, &held), "held");
    assert_eq!(recv(&mut a2, &held2), "held2");
    assert_eq!(recv(&mut b2, &send(&mut a2, "4")), "4");
    assert_eq!(recv(&mut a2, &send(&mut b2, "5")), "5");
    assert_eq!(a2.skipped_len(), 0);
}

#[test]
fn a_stored_ratchet_starts_with_its_format_byte_and_rejects_other_formats_and_truncation() {
    let Pair { alice, bob, .. } = handshake(false, false);
    for rc in [&alice, &bob] {
        let bytes = rc.to_bytes().unwrap();
        assert_eq!(bytes[0], 1);
        let mut other = bytes.clone();
        other[0] = 2;
        assert!(Ratchet::from_bytes(&other).is_err());
        assert!(Ratchet::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Ratchet::from_bytes(&extra).is_err());
    }
}

#[test]
fn private_params_with_an_accepted_kem_round_trip() {
    use zaun_chat::ratchet::{
        CURRENT_E2E_VERSION, PqSupport, generate_rcv_params, generate_snd_params, reply_kem,
    };
    let (_, rcv) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport::ON).unwrap();
    let use_kem = reply_kem(rcv.kem_key(), PqSupport::ON);
    let (private, _) = generate_snd_params(CURRENT_E2E_VERSION, use_kem.as_ref()).unwrap();
    assert_eq!(
        PrivParams::from_bytes(&private.to_bytes().unwrap()).unwrap(),
        private
    );
}
