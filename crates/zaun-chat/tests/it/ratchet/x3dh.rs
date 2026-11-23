//! X3DH agreement for every PQ combination, and the failures.
use super::support::handshake;
use zaun_chat::ratchet::{
    CURRENT_E2E_VERSION, PqSupport, RatchetError, UseKem, generate_rcv_params, generate_snd_params,
    pq_x3dh_rcv, pq_x3dh_snd, reply_kem,
};

#[test]
fn both_parties_agree_on_keys_and_associated_data_for_every_pq_combination() {
    let off = handshake(false, false);
    assert!(off.alice_init.kem_accepted.is_none());
    assert!(!off.alice.pq_support().0);
    let declined = handshake(true, false);
    assert!(declined.alice_init.kem_accepted.is_none() && !declined.alice.pq_support().0);
    let proposed_in_reply = handshake(false, true);
    assert!(proposed_in_reply.bob_init.kem_accepted.is_none());
    assert!(proposed_in_reply.bob.pq_support().0);
    let acc = handshake(true, true);
    let (a, b) = (
        acc.alice_init.kem_accepted.unwrap(),
        acc.bob_init.kem_accepted.unwrap(),
    );
    assert_eq!(a.shared, b.shared);
    assert_eq!(acc.alice.verify_code_pq(), acc.bob.verify_code_pq());
    assert_eq!(acc.alice.associated_data(), acc.bob.associated_data());
    assert_eq!(acc.alice.associated_data().len(), 112);
}

#[test]
fn a_substituted_kem_key_gives_different_verification_codes_but_the_same_associated_data() {
    let (a_priv, a_params) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport::ON).unwrap();
    let (_, mallory) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport::ON).unwrap();
    let use_kem = UseKem::Accept(mallory.kem_key().unwrap().clone());
    let (b_priv, b_params) = generate_snd_params(CURRENT_E2E_VERSION, Some(&use_kem)).unwrap();
    let (bi, _) = pq_x3dh_snd(&b_priv, &a_params).unwrap();
    // Alice cannot decapsulate: the ciphertext is for Mallory's key, so she gets an unrelated secret.
    let (ai, _) = pq_x3dh_rcv(&a_priv, &b_params).unwrap();
    assert_eq!(ai.assoc_data, bi.assoc_data);
    assert_ne!(ai.verify_code_pq, bi.verify_code_pq);
}

#[test]
fn accepting_a_kem_key_that_was_never_proposed_is_a_kem_state_error() {
    let (a_priv, a_params) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport::OFF).unwrap();
    let (_, other) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport::ON).unwrap();
    let use_kem = reply_kem(other.kem_key(), PqSupport::ON);
    let (b_priv, b_params) = generate_snd_params(CURRENT_E2E_VERSION, use_kem.as_ref()).unwrap();
    assert_eq!(
        pq_x3dh_snd(&b_priv, &a_params).unwrap_err(),
        RatchetError::KemState
    );
    assert_eq!(
        pq_x3dh_rcv(&a_priv, &b_params).unwrap_err(),
        RatchetError::KemState
    );
}
