//! X448 on raw key bytes (Crypto.hs `X448` instances, nook keys.ts).
use super::CryptoError;

pub(super) fn public(secret: &[u8; 56]) -> Result<[u8; 56], CryptoError> {
    x448::x448(*secret, x448::X448_BASEPOINT_BYTES).ok_or(CryptoError::Dh)
}

/// Fails on a low-order public key, whose output would be all zeros.
pub(super) fn dh(public: &[u8; 56], secret: &[u8; 56]) -> Result<[u8; 56], CryptoError> {
    x448::x448(*secret, *public).ok_or(CryptoError::Dh)
}
