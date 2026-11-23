//! NaCl crypto_box over an X25519 secret, and the SMP command authenticator (Crypto.hs `cbEncrypt`, `cbDecrypt`, `cbAuthenticate`, `cbVerify`).
use subtle::ConstantTimeEq;

use super::keys::dh;
use super::{
    CbNonce, CryptoError, DhSecret, PrivateKey, PublicKey, crypto_box, crypto_box_open, pad,
    sha512, unpad,
};

/// 64 (SHA-512) + 16 (tag).
pub const CB_AUTHENTICATOR_LEN: usize = 80;

pub fn cb_encrypt(
    secret: &DhSecret,
    nonce: &CbNonce,
    msg: &[u8],
    padded_len: usize,
) -> Result<Vec<u8>, CryptoError> {
    Ok(crypto_box(
        secret.as_x25519()?,
        nonce,
        &pad(msg, padded_len)?,
    ))
}

pub fn cb_encrypt_no_pad(
    secret: &DhSecret,
    nonce: &CbNonce,
    msg: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    Ok(crypto_box(secret.as_x25519()?, nonce, msg))
}

/// Rejects an all-zero secret, which a low-order public key gives.
pub fn cb_decrypt(
    secret: &DhSecret,
    nonce: &CbNonce,
    packet: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let raw = secret.as_x25519()?;
    if bool::from(raw.ct_eq(&[0u8; 32])) {
        return Err(CryptoError::CbDecrypt);
    }
    let padded = crypto_box_open(raw, nonce, packet)?;
    unpad(&padded).map(<[u8]>::to_vec)
}

pub fn cb_decrypt_no_pad(
    secret: &DhSecret,
    nonce: &CbNonce,
    packet: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    crypto_box_open(secret.as_x25519()?, nonce, packet)
}

/// `cbAuthenticate k pk nonce msg`: a box of `sha512(msg)` under DH(k, pk).
pub fn cb_authenticate(
    k: &PublicKey,
    pk: &PrivateKey,
    nonce: &CbNonce,
    msg: &[u8],
) -> Result<[u8; CB_AUTHENTICATOR_LEN], CryptoError> {
    let boxed = cb_encrypt_no_pad(&dh(k, pk)?, nonce, &sha512(msg))?;
    boxed.try_into().map_err(|_| CryptoError::InvalidMsg)
}

/// `cbVerify`; any failure is `false`.
#[must_use]
pub fn cb_verify(
    k: &PublicKey,
    pk: &PrivateKey,
    nonce: &CbNonce,
    authenticator: &[u8],
    authorized: &[u8],
) -> bool {
    dh(k, pk)
        .and_then(|secret| cb_decrypt_no_pad(&secret, nonce, authenticator))
        .is_ok_and(|digest| bool::from(digest.ct_eq(&sha512(authorized))))
}
