//! Signature checks on certificates and CertificateVerify: Ed25519 and Ed448 only.
use super::der::{certificate_public_key, parse_certificate};
use super::error::TlsError;
use crate::crypto::{Alg, PublicKey, Signature};

pub(super) const SIG_ED25519: u16 = 0x0807;
pub(super) const SIG_ED448: u16 = 0x0808;

fn verify_with(key: &PublicKey, sig: &[u8], msg: &[u8]) -> bool {
    Signature::from_raw(key.alg(), sig).is_ok_and(|s| key.verify(&s, msg))
}

/// The server's CertificateVerify, made with the leaf key under the scheme it announced.
pub(super) fn check_certificate_verify(
    leaf: &[u8],
    scheme: u16,
    signature: &[u8],
    signed: &[u8],
) -> Result<(), TlsError> {
    let key = certificate_public_key(leaf)?;
    let expected = match scheme {
        SIG_ED25519 => Alg::Ed25519,
        SIG_ED448 => Alg::Ed448,
        _ => return Err(TlsError::Unsupported("CertificateVerify signature scheme")),
    };
    if key.alg() != expected {
        return Err(TlsError::Illegal(
            "signature scheme does not match the leaf key",
        ));
    }
    verify_with(&key, signature, signed)
        .then_some(())
        .ok_or(TlsError::BadCertificateVerify)
}

/// Whether `issuer`'s key signed `cert` (false for an unsupported key or a bad signature).
pub(super) fn certificate_signed_by(cert: &[u8], issuer: &[u8]) -> bool {
    let (Ok(c), Ok(key)) = (parse_certificate(cert), certificate_public_key(issuer)) else {
        return false;
    };
    verify_with(&key, c.signature, c.tbs)
}
