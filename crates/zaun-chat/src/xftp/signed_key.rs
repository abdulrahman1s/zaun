//! The server's DH key signed by its leaf certificate: `SignedExact PubKey` (Crypto.hs `verifyX509`; nook crypto/identity.ts `parseSignedKey`).
//!
//! TODO(crypto): a copy of smp/signed_key.rs; one shared home next to the X.509 helpers of `crate::tls` when they export a DER reader.
use crate::crypto::{PublicKey, Signature};
use crate::tls::certificate_public_key;

use super::error::HandshakeError;

/// One DER element: its tag, its whole encoding and its content.
struct Tlv<'a> {
    tag: u8,
    raw: &'a [u8],
    value: &'a [u8],
}

fn tlv(buf: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    let (&tag, rest) = buf.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, head) = if first & 0x80 == 0 {
        (usize::from(first), 2)
    } else {
        let n = usize::from(first & 0x7f);
        if !(1..=3).contains(&n) {
            return None;
        }
        (
            rest.get(..n)?
                .iter()
                .fold(0usize, |a, b| a << 8 | usize::from(*b)),
            2 + n,
        )
    };
    let raw = buf.get(..head + len)?;
    Some((
        Tlv {
            tag,
            raw,
            value: &raw[head..],
        },
        &buf[head + len..],
    ))
}

fn children(mut buf: &[u8]) -> Option<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        let (t, rest) = tlv(buf)?;
        out.push(t);
        buf = rest;
    }
    Some(out)
}

/// Checks `signed` against the leaf certificate and returns the X25519 key inside.
pub fn verify_signed_key(leaf_cert: &[u8], signed: &[u8]) -> Result<PublicKey, HandshakeError> {
    let bad = HandshakeError::BadAuth;
    let (outer, rest) = tlv(signed).ok_or(bad)?;
    if outer.tag != 0x30 || !rest.is_empty() {
        return Err(bad);
    }
    let parts = children(outer.value).ok_or(bad)?;
    let [object, _alg, sig] = parts.as_slice() else {
        return Err(bad);
    };
    let sig_bytes = match (sig.tag, sig.value.split_first()) {
        (0x03, Some((0, s))) => s,
        _ => return Err(bad),
    };
    // x509's objectToSignedExact may wrap the SubjectPublicKeyInfo in one more SEQUENCE; the signature covers `object.raw` either way.
    let spki = if object.raw.len() == 44 {
        object.raw
    } else {
        children(object.value).ok_or(bad)?.first().ok_or(bad)?.raw
    };
    let key = PublicKey::from_der(spki).map_err(|_| bad)?;
    if !matches!(key, PublicKey::X25519(_)) {
        return Err(bad);
    }
    let signer = certificate_public_key(leaf_cert).map_err(|_| bad)?;
    let signature = Signature::from_raw(signer.alg(), sig_bytes).map_err(|_| bad)?;
    if !signer.verify(&signature, object.raw) {
        return Err(bad);
    }
    Ok(key)
}
