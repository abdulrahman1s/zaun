//! XChaCha20-Poly1305 for the store's encrypted columns: `nonce(24) || ciphertext || tag` (nook uses noble `xchacha20poly1305`).
use chacha20poly1305::{
    XChaCha20Poly1305,
    aead::{Aead, KeyInit, Payload},
};

use super::{CryptoError, random_array};

const NONCE_LEN: usize = 24;

/// Seals with a fresh random nonce, which is written first.
pub fn xchacha_seal(key: &[u8; 32], aad: &[u8], msg: &[u8]) -> Result<Vec<u8>, CryptoError> {
    xchacha_seal_with_nonce(key, &random_array()?, aad, msg)
}

/// For known-answer tests; callers use `xchacha_seal`, a nonce must never repeat under a key.
pub fn xchacha_seal_with_nonce(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    msg: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let sealed = XChaCha20Poly1305::new(&(*key).into())
        .encrypt(&(*nonce).into(), Payload { msg, aad })
        .map_err(|_| CryptoError::Aead)?;
    let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn xchacha_open(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let (nonce, body) = sealed
        .split_first_chunk::<NONCE_LEN>()
        .ok_or(CryptoError::Aead)?;
    XChaCha20Poly1305::new(&(*key).into())
        .decrypt(&(*nonce).into(), Payload { msg: body, aad })
        .map_err(|_| CryptoError::Aead)
}
