//! Limits and failures: too many skipped, earlier and duplicate messages, tampering, eviction.
use super::support::{Pair, handshake, recv, recv_err, send};
use zaun_chat::ratchet::{MAX_SKIP, MAX_SKIPPED_MSG_KEYS, RatchetError};

#[test]
fn a_jump_over_more_than_max_skip_messages_is_refused_and_the_state_survives() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    assert_eq!(recv(&mut alice, &send(&mut bob, "first")), "first");
    let far: Vec<_> = (0..MAX_SKIP + 3)
        .map(|i| send(&mut bob, &format!("x{i}")))
        .collect();
    let before = alice.clone();
    let err = recv_err(&mut alice, far.last().unwrap());
    assert_eq!(err, RatchetError::TooManySkipped(MAX_SKIP + 3));
    assert_eq!(alice, before, "a refused jump changes nothing");
    assert_eq!(recv(&mut alice, &far[0]), "x0");
}

#[test]
fn a_duplicate_and_an_earlier_message_are_told_apart() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    let m: Vec<_> = (0..5).map(|i| send(&mut bob, &format!("m{i}"))).collect();
    assert_eq!(recv(&mut alice, &m[0]), "m0");
    assert_eq!(recv_err(&mut alice, &m[0]), RatchetError::DuplicateMessage);
    assert_eq!(recv(&mut alice, &m[3]), "m3");
    assert_eq!(recv_err(&mut alice, &m[3]), RatchetError::DuplicateMessage);
    assert_eq!(recv_err(&mut alice, &m[0]), RatchetError::EarlierMessage(3));
    assert_eq!(recv(&mut alice, &m[1]), "m1");
    assert_eq!(recv_err(&mut alice, &m[1]), RatchetError::EarlierMessage(2));
}

#[test]
fn a_tampered_body_fails_but_later_messages_still_decrypt() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    assert_eq!(recv(&mut alice, &send(&mut bob, "first")), "first");
    let (a, b, c) = (
        send(&mut bob, "a"),
        send(&mut bob, "b"),
        send(&mut bob, "c"),
    );
    let mut bad = a.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert_eq!(recv_err(&mut alice, &bad), RatchetError::AesDecrypt);
    assert_eq!(recv(&mut alice, &b), "b");
    assert_eq!(recv(&mut alice, &c), "c");
}

#[test]
fn a_tampered_header_or_garbage_is_not_accepted_and_changes_nothing() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    assert_eq!(recv(&mut alice, &send(&mut bob, "first")), "first");
    let m = send(&mut bob, "second");
    let mut bad = m.clone();
    bad[10] ^= 1;
    let before = alice.clone();
    assert_eq!(recv_err(&mut alice, &bad), RatchetError::HeaderKey);
    assert!(matches!(
        recv_err(&mut alice, &[1, 2, 3]),
        RatchetError::Header(_)
    ));
    assert!(matches!(recv_err(&mut alice, &[]), RatchetError::Header(_)));
    assert_eq!(alice, before);
    assert_eq!(recv(&mut alice, &m), "second");
}

#[test]
fn the_receiver_without_a_sending_chain_cannot_encrypt() {
    let Pair { mut alice, .. } = handshake(false, false);
    assert_eq!(
        alice.encrypt(b"x", 100, None).unwrap_err(),
        RatchetError::State
    );
    assert!(!alice.has_sending_chain());
}

#[test]
fn a_message_too_large_for_the_padding_is_refused_before_the_chain_moves() {
    let Pair { mut bob, .. } = handshake(false, false);
    let before = bob.clone();
    assert_eq!(
        bob.encrypt(&[0; 200], 100, None).unwrap_err(),
        RatchetError::LargeMsg
    );
    assert_eq!(bob, before);
}

#[test]
fn skipped_keys_are_capped_and_the_oldest_are_dropped() {
    let Pair {
        mut alice, mut bob, ..
    } = handshake(false, false);
    assert_eq!(recv(&mut alice, &send(&mut bob, "first")), "first");
    let chunk = MAX_SKIP as usize;
    let mut held = Vec::new();
    // each jump of `chunk` skips `chunk` keys; deliver the newest of each batch only
    for _ in 0..5 {
        let batch: Vec<_> = (0..=chunk)
            .map(|i| send(&mut bob, &format!("m{i}")))
            .collect();
        recv(&mut alice, batch.last().unwrap());
        held.push(batch);
    }
    assert_eq!(alice.skipped_len(), MAX_SKIPPED_MSG_KEYS.min(5 * chunk));
    // the oldest batch lost keys, the newest one did not
    assert!(
        held[0]
            .first()
            .map(|m| alice.clone().decrypt(m).is_err())
            .unwrap()
    );
    assert_eq!(recv(&mut alice, &held[4][0]), "m0");
}
