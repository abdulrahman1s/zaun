//! sntrup761 vectors made by simplexmq's C (.claude/spikes/kem harness): the Rust KEM and the ratchet agree with it.
use crate::hex::unhex;
use std::collections::HashMap;
use zaun_chat::crypto::{Alg, KeyPair};
use zaun_chat::ratchet::{
    CURRENT_E2E_VERSION, E2eParams, KemCiphertext, KemKeyPair, KemParams, KemPublicKey,
    KemSecretKey, KemSharedSecret, PqSupport, PrivKem, PrivParams, Ratchet, RatchetVersions,
    kem_decapsulate, pq_x3dh_rcv, pq_x3dh_snd,
};

struct Triple {
    keys: KemKeyPair,
    ct: KemCiphertext,
    ss: KemSharedSecret,
}

fn vectors() -> HashMap<String, Vec<u8>> {
    include_str!("kem_vectors.txt")
        .lines()
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_owned(), unhex(v))
        })
        .collect()
}

/// A key pair, and a ciphertext to it with its secret, all made by the C.
fn triple(name: &str) -> Triple {
    let v = vectors();
    let get = |k: &str| v[&format!("{name}_{k}")].as_slice();
    Triple {
        keys: KemKeyPair {
            public: KemPublicKey::from_bytes(get("pk")).unwrap(),
            secret: KemSecretKey::from_bytes(get("sk")).unwrap(),
        },
        ct: KemCiphertext::from_bytes(get("ct")).unwrap(),
        ss: KemSharedSecret::from_bytes(get("ss")).unwrap(),
    }
}

#[test]
fn decapsulating_a_ciphertext_made_by_the_reference_c_gives_its_shared_secret() {
    for name in ["a", "b"] {
        let t = triple(name);
        assert_eq!(kem_decapsulate(&t.ct, &t.keys.secret), t.ss);
    }
}

#[test]
fn a_ciphertext_for_another_key_is_rejected_implicitly_with_a_different_secret() {
    let (a, b) = (triple("a"), triple("b"));
    assert_ne!(kem_decapsulate(&a.ct, &b.keys.secret), a.ss);
}

fn x448_priv() -> PrivParams {
    let (k1, k2) = (
        KeyPair::generate(Alg::X448).unwrap(),
        KeyPair::generate(Alg::X448).unwrap(),
    );
    PrivParams {
        pk1: k1.private,
        pk2: k2.private,
        kem: None,
    }
}

#[test]
fn x3dh_with_reference_kem_vectors_agrees_on_the_c_shared_secret() {
    let (a, b) = (triple("a"), triple("b"));
    // Alice proposes key a; Bob answers with the C ciphertext to it, and offers key b.
    let mut alice = x448_priv();
    alice.kem = Some(PrivKem::Proposed(a.keys.clone()));
    let rcv = alice.rcv_params(CURRENT_E2E_VERSION).unwrap();
    let mut bob = x448_priv();
    bob.kem = Some(PrivKem::Accepted {
        ct: a.ct.clone(),
        shared: a.ss.clone(),
        keys: b.keys.clone(),
    });
    let reply = E2eParams::new(
        CURRENT_E2E_VERSION,
        bob.pk1.public().unwrap(),
        bob.pk2.public().unwrap(),
        Some(KemParams::Accepted {
            ct: a.ct.clone(),
            key: b.keys.public.clone(),
        }),
    )
    .unwrap();
    let (bob_init, bob_keys) = pq_x3dh_snd(&bob, &rcv).unwrap();
    let (alice_init, alice_keys) = pq_x3dh_rcv(&alice, &reply).unwrap();
    super::support::assert_same_init(&alice_init, &bob_init);
    assert_eq!(alice_init.kem_accepted.as_ref().unwrap().shared, a.ss);
    assert_eq!(alice_keys, Some(a.keys.clone()));
    assert_eq!(bob_keys, Some(b.keys.clone()));

    // The conversation runs on those keys, through a PQ ratchet step each way.
    let versions = RatchetVersions::new(CURRENT_E2E_VERSION);
    let dhrs = KeyPair::generate(Alg::X448).unwrap().private;
    let mut bob_rc = Ratchet::init_snd(
        versions,
        rcv.params().k2.clone(),
        dhrs,
        (bob_init, bob_keys),
    )
    .unwrap();
    let mut alice_rc = Ratchet::init_rcv(
        versions,
        alice.pk2.clone(),
        (alice_init, alice_keys),
        PqSupport::ON,
    );
    for i in 0..3 {
        let m = bob_rc
            .encrypt(format!("b{i}").as_bytes(), 100, None)
            .unwrap();
        assert_eq!(alice_rc.decrypt(&m).unwrap(), format!("b{i}").as_bytes());
        let m = alice_rc
            .encrypt(format!("a{i}").as_bytes(), 100, None)
            .unwrap();
        assert_eq!(bob_rc.decrypt(&m).unwrap(), format!("a{i}").as_bytes());
    }
}
