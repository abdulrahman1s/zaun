//! Minimal DER certificates signed for real, so chain checks have something to check.
use crate::crypto::{Alg, KeyPair, PublicKey, encode_public_der};

fn der(tag: u8, parts: &[&[u8]]) -> Vec<u8> {
    let body = parts.concat();
    let mut out = vec![tag];
    match body.len() {
        n if n < 128 => out.push(n as u8),
        n if n < 256 => out.extend([0x81, n as u8]),
        n => out.extend([0x82, (n >> 8) as u8, n as u8]),
    }
    out.extend(body);
    out
}

fn signature_algorithm(alg: Alg) -> Vec<u8> {
    let oid = if alg == Alg::Ed448 { 0x71 } else { 0x70 };
    der(0x30, &[&der(0x06, &[&[0x2b, 0x65, oid]])])
}

/// A certificate for `subject`, signed by `issuer` (the same pair for a self-signed one).
pub(in crate::tls) fn make_cert(subject: &PublicKey, issuer: &KeyPair, serial: u8) -> Vec<u8> {
    let alg = signature_algorithm(issuer.public.alg());
    let empty = der(0x30, &[]);
    let tbs = der(
        0x30,
        &[
            &der(0xa0, &[&der(0x02, &[&[2]])]),
            &der(0x02, &[&[serial]]),
            &alg,
            &empty,
            &empty,
            &empty,
            &encode_public_der(subject.alg(), subject.as_bytes()),
        ],
    );
    let signature = issuer.private.sign(&tbs).map(|s| s.as_bytes().to_vec());
    let signature = signature.unwrap_or_default();
    der(0x30, &[&tbs, &alg, &der(0x03, &[&[0], &signature])])
}
