//! Out-of-order and skipped delivery (RatchetTests.hs `testSkippedMessages`; nook's skipped and fuzz tests).
use super::support::{MODES, Pair, handshake, recv, recv_err, reload, send};
use zaun_chat::ratchet::RatchetError;

#[test]
fn skipped_and_out_of_order_messages_decrypt_across_ratchet_steps() {
    for (name, invite, join) in MODES {
        let Pair {
            mut alice, mut bob, ..
        } = handshake(invite, join);
        assert_eq!(recv(&mut alice, &send(&mut bob, "b1")), "b1", "{name}");
        let b2 = send(&mut bob, "b2");
        let b3 = send(&mut bob, "b3");
        let b4 = send(&mut bob, "b4");
        assert_eq!(recv(&mut bob, &send(&mut alice, "a5")), "a5");
        let b5 = send(&mut bob, "b5");
        let b6 = send(&mut bob, "b6");
        assert_eq!(recv(&mut alice, &send(&mut bob, "b7")), "b7");
        let b8 = send(&mut bob, "b8");
        let b9 = send(&mut bob, "b9");
        assert_eq!(recv(&mut bob, &send(&mut alice, "a10")), "a10");
        let b11 = send(&mut bob, "b11");
        let b12 = send(&mut bob, "b12");
        assert_eq!(recv(&mut bob, &send(&mut alice, "a14")), "a14");
        assert_eq!(recv(&mut alice, &b12), "b12");
        assert_eq!(recv(&mut alice, &b2), "b2");
        assert!(matches!(
            recv_err(&mut alice, &b2),
            RatchetError::HeaderKey
                | RatchetError::DuplicateMessage
                | RatchetError::EarlierMessage(_)
        ));
        assert_eq!(recv(&mut bob, &send(&mut alice, "a15")), "a15");
        let a16 = send(&mut bob, "a16");
        let a17 = send(&mut bob, "a17");
        for (m, t) in [
            (&b8, "b8"),
            (&b3, "b3"),
            (&b4, "b4"),
            (&b5, "b5"),
            (&b6, "b6"),
        ] {
            assert_eq!(recv(&mut alice, m), t);
        }
        assert_eq!(recv(&mut bob, &send(&mut alice, "a18")), "a18");
        assert_eq!(recv(&mut alice, &a16), "a16");
        assert_eq!(recv(&mut alice, &a17), "a17");
        assert_eq!(recv(&mut alice, &b9), "b9");
        assert_eq!(recv(&mut alice, &b11), "b11");
        assert_eq!(alice.skipped_len(), 0, "{name}");
    }
}

#[test]
fn random_sends_and_deliveries_with_state_stored_between_steps_all_decrypt() {
    for (name, invite, join) in MODES {
        let Pair {
            mut alice, mut bob, ..
        } = handshake(invite, join);
        let mut seed = 12345u32;
        let mut rnd = |n: usize| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345) & 0x7fff_ffff;
            seed as usize % n
        };
        // (to alice?, wire, text); bob starts: alice has no sending chain yet
        let mut inflight: Vec<(bool, Vec<u8>, String)> = Vec::new();
        let steps = if invite || join { 40 } else { 150 };
        for i in 0..steps {
            let action = rnd(10);
            if action < 3 || (action < 5 && !alice.has_sending_chain()) {
                let text = format!("b{i}");
                inflight.push((true, send(&mut bob, &text), text));
            } else if action < 5 {
                let text = format!("a{i}");
                inflight.push((false, send(&mut alice, &text), text));
            } else if !inflight.is_empty() {
                let (to_alice, wire, text) = inflight.remove(rnd(inflight.len()));
                let r = if to_alice { &mut alice } else { &mut bob };
                assert_eq!(recv(r, &wire), text, "{name}");
            }
            if rnd(4) == 0 {
                alice = reload(&alice);
            }
            if rnd(4) == 0 {
                bob = reload(&bob);
            }
        }
        while !inflight.is_empty() {
            let (to_alice, wire, text) = inflight.remove(rnd(inflight.len()));
            let r = if to_alice { &mut alice } else { &mut bob };
            assert_eq!(recv(r, &wire), text, "{name}");
        }
        assert_eq!((alice.skipped_len(), bob.skipped_len()), (0, 0), "{name}");
    }
}

#[test]
fn many_messages_in_one_direction_are_all_recovered_after_the_last_arrives_first() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    let msgs: Vec<_> = (0..300).map(|i| send(&mut bob, &format!("m{i}"))).collect();
    assert_eq!(recv(&mut alice, &msgs[299]), "m299");
    assert_eq!(alice.skipped_len(), 299);
    for (i, m) in msgs.iter().enumerate().take(299) {
        assert_eq!(recv(&mut alice, m), format!("m{i}"));
    }
    assert_eq!(alice.skipped_len(), 0);
}
