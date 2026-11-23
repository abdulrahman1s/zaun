//! A tiny X.509 builder: Ed25519 certificates with just the fields the client reads, and the server's signed DH key.
use crate::crypto::{Alg, KeyPair, PublicKey, key_hash};

use super::support::keypair;

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    match content.len() {
        n if n < 128 => out.push(n as u8),
        n => out.extend([0x82, (n >> 8) as u8, n as u8]),
    }
    out.extend(content);
    out
}

const ED25519_ALG: [u8; 7] = [0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70];

fn sign_over(tbs: &[u8], signer: &KeyPair) -> Vec<u8> {
    let sig = signer.private.sign(tbs).unwrap();
    der(
        0x30,
        &[
            tbs,
            &ED25519_ALG,
            &der(0x03, &[&[0u8][..], sig.as_bytes()].concat()),
        ]
        .concat(),
    )
}

/// A certificate for `subject`, signed by `issuer` (itself for a CA).
pub fn certificate(subject: &PublicKey, issuer: &KeyPair) -> Vec<u8> {
    let fields = [
        der(0x02, &[1]),
        ED25519_ALG.to_vec(),
        der(0x30, &[]),
        der(0x30, &[]),
        der(0x30, &[]),
        subject.to_der(),
    ]
    .concat();
    sign_over(&der(0x30, &fields), issuer)
}

/// A server identity: CA, leaf, their chain, the key hash an address would carry.
pub struct Identity {
    pub leaf: KeyPair,
    pub chain: Vec<Vec<u8>>,
    pub key_hash: [u8; 32],
}

pub fn identity() -> Identity {
    let (ca, leaf) = (keypair(Alg::Ed25519), keypair(Alg::Ed25519));
    let chain = vec![certificate(&leaf.public, &ca), certificate(&ca.public, &ca)];
    let key_hash = key_hash(&chain[1]);
    Identity {
        leaf,
        chain,
        key_hash,
    }
}

/// `SignedExact PubKey`: the DH key's SPKI, an algorithm identifier and the leaf's signature over the SPKI.
pub fn signed_key(dh: &PublicKey, leaf: &KeyPair) -> Vec<u8> {
    let object = dh.to_der();
    let sig = leaf.private.sign(&object).unwrap();
    der(
        0x30,
        &[
            object,
            ED25519_ALG.to_vec(),
            der(0x03, &[&[0u8][..], sig.as_bytes()].concat()),
        ]
        .concat(),
    )
}
