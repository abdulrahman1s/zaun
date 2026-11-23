//! X.509 SubjectPublicKeyInfo and PKCS#8 DER of the four key kinds, byte for byte as SimpleX writes them (RFC 8410; nook keys.ts).
use zeroize::Zeroizing;

use super::{Alg, CryptoError};

/// `SEQUENCE { OID 1.3.101` up to the algorithm byte.
const ALG_ID: [u8; 6] = [0x30, 0x05, 0x06, 0x03, 0x2b, 0x65];

/// `SEQUENCE { SEQUENCE { OID }, BIT STRING { 0, key } }`.
#[must_use]
pub fn encode_public_der(alg: Alg, key: &[u8]) -> Vec<u8> {
    let n = key.len();
    let mut out = vec![0x30, (10 + n) as u8];
    out.extend_from_slice(&ALG_ID);
    out.extend_from_slice(&[alg.oid(), 0x03, (n + 1) as u8, 0x00]);
    out.extend_from_slice(key);
    out
}

/// Only the canonical encoding is accepted: the input must equal what `encode_public_der` writes.
pub fn decode_public_der(der: &[u8]) -> Result<(Alg, &[u8]), CryptoError> {
    let bad = CryptoError::BadKey("public key DER");
    let alg = der
        .get(8)
        .and_then(|b| Alg::from_oid(*b))
        .ok_or(bad.clone())?;
    let key = der.get(12..).ok_or(bad.clone())?;
    (key.len() == alg.public_len() && encode_public_der(alg, key) == der)
        .then_some((alg, key))
        .ok_or(bad)
}

/// `SEQUENCE { INTEGER 0, SEQUENCE { OID }, OCTET STRING { OCTET STRING { key } } }`.
#[must_use]
pub fn encode_private_der(alg: Alg, key: &[u8]) -> Zeroizing<Vec<u8>> {
    let n = key.len();
    let mut out = Zeroizing::new(vec![0x30, (14 + n) as u8, 0x02, 0x01, 0x00]);
    out.extend_from_slice(&ALG_ID);
    out.extend_from_slice(&[alg.oid(), 0x04, (n + 2) as u8, 0x04, n as u8]);
    out.extend_from_slice(key);
    out
}

pub fn decode_private_der(der: &[u8]) -> Result<(Alg, Zeroizing<Vec<u8>>), CryptoError> {
    let bad = CryptoError::BadKey("private key DER");
    let alg = der
        .get(11)
        .and_then(|b| Alg::from_oid(*b))
        .ok_or(bad.clone())?;
    let key = der.get(16..).ok_or(bad.clone())?;
    if key.len() != alg.private_len() || *encode_private_der(alg, key) != der {
        return Err(bad);
    }
    Ok((alg, Zeroizing::new(key.to_vec())))
}
