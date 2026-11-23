//! Just enough DER to read an X.509 certificate: tbs, key and signature (nook sx/tls/der.ts).
use thiserror::Error;

use crate::crypto::{PublicKey, decode_public_der};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct CertError(pub(super) &'static str);

const SEQUENCE: u8 = 0x30;
const BIT_STRING: u8 = 0x03;
const VERSION: u8 = 0xa0;

struct Tlv<'a> {
    tag: u8,
    raw: &'a [u8],
    value: &'a [u8],
}

fn tlv(b: &[u8]) -> Result<(Tlv<'_>, &[u8]), CertError> {
    let (&tag, rest) = b.split_first().ok_or(CertError("der: truncated"))?;
    if tag & 0x1f == 0x1f {
        return Err(CertError("der: high tag numbers unsupported"));
    }
    let (&first, rest) = rest.split_first().ok_or(CertError("der: truncated"))?;
    let (len, head) = if first & 0x80 == 0 {
        (usize::from(first), 2)
    } else {
        let n = usize::from(first & 0x7f);
        if !(1..=3).contains(&n) {
            return Err(CertError("der: unsupported length form"));
        }
        let bytes = rest.get(..n).ok_or(CertError("der: truncated"))?;
        (bytes.iter().fold(0, |a, x| a << 8 | usize::from(*x)), 2 + n)
    };
    let raw = b
        .get(..head + len)
        .ok_or(CertError("der: length exceeds input"))?;
    Ok((
        Tlv {
            tag,
            raw,
            value: &raw[head..],
        },
        &b[head + len..],
    ))
}

fn children(mut b: &[u8]) -> Result<Vec<Tlv<'_>>, CertError> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let (t, rest) = tlv(b)?;
        out.push(t);
        b = rest;
    }
    Ok(out)
}

/// The signed part, the SubjectPublicKeyInfo and the signature of a certificate.
pub(super) struct Certificate<'a> {
    pub(super) tbs: &'a [u8],
    pub(super) spki: &'a [u8],
    pub(super) signature: &'a [u8],
}

pub(super) fn parse_certificate(der: &[u8]) -> Result<Certificate<'_>, CertError> {
    let (outer, rest) = tlv(der)?;
    if outer.tag != SEQUENCE || !rest.is_empty() {
        return Err(CertError("x509: not a single SEQUENCE"));
    }
    let parts = children(outer.value)?;
    let [tbs, _alg, sig] = parts.as_slice() else {
        return Err(CertError("x509: certificate needs three parts"));
    };
    if tbs.tag != SEQUENCE || sig.tag != BIT_STRING {
        return Err(CertError("x509: bad certificate parts"));
    }
    let fields = children(tbs.value)?;
    let skip = usize::from(fields.first().is_some_and(|f| f.tag == VERSION));
    // serialNumber, signature, issuer, validity, subject, subjectPublicKeyInfo
    let spki = fields
        .get(skip + 5)
        .filter(|f| f.tag == SEQUENCE)
        .ok_or(CertError("x509: no SubjectPublicKeyInfo"))?;
    let (&unused_bits, signature) = sig
        .value
        .split_first()
        .ok_or(CertError("x509: empty signature"))?;
    if unused_bits != 0 {
        return Err(CertError("x509: bad signature padding"));
    }
    Ok(Certificate {
        tbs: tbs.raw,
        spki: spki.raw,
        signature,
    })
}

/// The Ed25519, Ed448, X25519 or X448 key of a certificate; other key types are refused.
pub fn certificate_public_key(der: &[u8]) -> Result<PublicKey, CertError> {
    let spki = parse_certificate(der)?.spki;
    let (alg, key) =
        decode_public_der(spki).map_err(|_| CertError("x509: unsupported public key"))?;
    PublicKey::from_raw(alg, key).map_err(|_| CertError("x509: bad public key length"))
}
