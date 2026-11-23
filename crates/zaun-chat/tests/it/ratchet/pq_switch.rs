//! Turning PQ encryption on and off in a running conversation (RatchetTests.hs `testEnableKEM`, `testDisableEnableKEM`).
use super::support::{PADDED, Pair, handshake, recv, send, send_pq};
use zaun_chat::ratchet::{PqSupport, padded_header_len};

#[test]
fn pq_is_enabled_mid_conversation_then_disabled_then_enabled_again() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    assert!(!bob.pq_support().0);
    assert_eq!(
        recv(&mut alice, &send_pq(&mut bob, "hello alice", false)),
        "hello alice"
    );
    assert_eq!(
        recv(&mut bob, &send_pq(&mut alice, "hello bob", false)),
        "hello bob"
    );
    for i in 0..6 {
        assert_eq!(
            recv(&mut alice, &send_pq(&mut bob, &format!("e-b{i}"), true)),
            format!("e-b{i}")
        );
        assert_eq!(
            recv(&mut bob, &send_pq(&mut alice, &format!("e-a{i}"), true)),
            format!("e-a{i}")
        );
    }
    assert!(alice.pq_support().0 && bob.pq_support().0);
    for r in [&alice, &bob] {
        assert!(r.pq_sending().0 && r.pq_receiving().0);
    }
    let big = send_pq(&mut bob, "pq header", true);
    assert_eq!(big.len(), 54 + padded_header_len(PqSupport::ON) + PADDED);
    assert_eq!(recv(&mut alice, &big), "pq header");
    for i in 0..6 {
        assert_eq!(
            recv(&mut bob, &send_pq(&mut alice, &format!("d-a{i}"), false)),
            format!("d-a{i}")
        );
        assert_eq!(
            recv(&mut alice, &send_pq(&mut bob, &format!("d-b{i}"), false)),
            format!("d-b{i}")
        );
    }
    for r in [&alice, &bob] {
        assert!(!r.pq_sending().0 && !r.pq_receiving().0);
        assert!(r.pq_support().0, "support is only ever turned on");
    }
    for i in 0..6 {
        assert_eq!(
            recv(&mut alice, &send_pq(&mut bob, &format!("r-b{i}"), true)),
            format!("r-b{i}")
        );
        assert_eq!(
            recv(&mut bob, &send_pq(&mut alice, &format!("r-a{i}"), true)),
            format!("r-a{i}")
        );
    }
    assert!(alice.pq_sending().0 && alice.pq_receiving().0);
}

#[test]
fn without_a_request_the_pq_setting_of_the_ratchet_is_kept() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(true, true);
    for i in 0..4 {
        assert_eq!(
            recv(&mut alice, &send(&mut bob, &format!("b{i}"))),
            format!("b{i}")
        );
        assert_eq!(
            recv(&mut bob, &send(&mut alice, &format!("a{i}"))),
            format!("a{i}")
        );
    }
    assert!(alice.pq_enabled().0 && bob.pq_enabled().0);
}
