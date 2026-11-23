use super::support::*;
use crate::crypto::{Alg, CbNonce, PublicKey, Signature, cb_verify, random_vec};
use crate::smp::{Authorizer, authorized};

fn setup() -> (crate::crypto::KeyPair, Vec<u8>) {
    (keypair(Alg::X25519), vec![5; 32])
}

#[test]
fn ed25519_authenticator_signs_the_session_prefixed_bytes() {
    let (server, session) = setup();
    let key = keypair(Alg::Ed25519);
    let a = Authorizer {
        session_id: &session,
        server_key: &server.public,
    };
    let auth = a
        .authenticate(Some(&key.private), &[1; 24], b"ent", b"SUB")
        .unwrap();
    assert_eq!(auth.len(), 64);
    let msg = authorized(&session, &[1; 24], b"ent", b"SUB").unwrap();
    assert!(
        key.public
            .verify(&Signature::from_raw(Alg::Ed25519, &auth).unwrap(), &msg)
    );
}

#[test]
fn x25519_authenticator_is_eighty_bytes_and_the_server_can_verify_it() {
    let (server, session) = setup();
    let key = keypair(Alg::X25519);
    let corr = random_vec(24).unwrap();
    let a = Authorizer {
        session_id: &session,
        server_key: &server.public,
    };
    let auth = a
        .authenticate(Some(&key.private), &corr, b"ent", b"SKEY x")
        .unwrap();
    assert_eq!(auth.len(), 80);
    let msg = authorized(&session, &corr, b"ent", b"SKEY x").unwrap();
    let nonce = CbNonce::from_slice(&corr).unwrap();
    assert!(cb_verify(&key.public, &server.private, &nonce, &auth, &msg));
    assert!(!cb_verify(
        &key.public,
        &server.private,
        &nonce,
        &auth,
        b"other"
    ));
}

#[test]
fn no_key_means_an_empty_authenticator() {
    let (server, session) = setup();
    let a = Authorizer {
        session_id: &session,
        server_key: &server.public,
    };
    assert!(
        a.authenticate(None, &[0; 24], b"", b"PING")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn x25519_authenticator_needs_a_24_byte_corr_id() {
    let (server, session) = setup();
    let a = Authorizer {
        session_id: &session,
        server_key: &server.public,
    };
    assert!(
        a.authenticate(Some(&private(Alg::X25519)), &[1; 5], b"e", b"SUB")
            .is_err()
    );
}

#[test]
fn x25519_authenticator_needs_an_x25519_server_key() {
    let session = vec![0; 32];
    let wrong: PublicKey = sample_pub_key();
    let a = Authorizer {
        session_id: &session,
        server_key: &wrong,
    };
    assert!(
        a.authenticate(Some(&private(Alg::X25519)), &[1; 24], b"e", b"SUB")
            .is_err()
    );
}
