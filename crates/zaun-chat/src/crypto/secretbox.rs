//! SimpleX crypto_box / secretbox: HSalsa20 pre-step then XSalsa20-Poly1305, `tag || ciphertext` (Crypto.hs `cryptoBox`, `sbEncrypt`; nook cryptobox.ts).
use cipher::consts::U10;
use salsa20::hsalsa;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::{CryptoError, SbState, pad, random_array, unpad};

/// A symmetric 32-byte secretbox key (Crypto.hs `SbKey`); also a chain key or a hybrid KEM secret.
#[derive(Clone)]
pub struct SbKey(Zeroizing<[u8; 32]>);

/// A 24-byte nonce (Crypto.hs `CbNonce`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CbNonce([u8; 24]);

impl SbKey {
    #[must_use]
    pub fn new(key: [u8; 32]) -> Self {
        Self(Zeroizing::new(key))
    }

    pub fn from_slice(b: &[u8]) -> Result<Self, CryptoError> {
        b.try_into()
            .map(Self::new)
            .map_err(|_| CryptoError::BadKey("secretbox key size"))
    }

    pub fn random() -> Result<Self, CryptoError> {
        random_array().map(Self::new)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for SbKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SbKey(..)")
    }
}

impl PartialEq for SbKey {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.0.ct_eq(&*other.0))
    }
}

impl Eq for SbKey {}

impl CbNonce {
    #[must_use]
    pub fn new(nonce: [u8; 24]) -> Self {
        Self(nonce)
    }

    pub fn from_slice(b: &[u8]) -> Result<Self, CryptoError> {
        b.try_into()
            .map(Self)
            .map_err(|_| CryptoError::BadKey("nonce size"))
    }

    /// `randomCbNonce`.
    pub fn random() -> Result<Self, CryptoError> {
        random_array().map(Self)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 24] {
        &self.0
    }
}

/// HSalsa20(secret, 0^16): the effective key for a raw DH output or an `SbKey` (nook `cryptoBoxKey`).
#[must_use]
pub fn box_key(secret: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    Zeroizing::new(hsalsa::<U10>(&(*secret).into(), &[0u8; 16].into()).into())
}

/// `cryptoBox secret nonce msg`: `tag(16) || ciphertext`, no padding.
#[must_use]
pub fn crypto_box(secret: &[u8; 32], nonce: &CbNonce, msg: &[u8]) -> Vec<u8> {
    let mut state = SbState::from_secret(secret, nonce);
    let mut body = msg.to_vec();
    state.encrypt(&mut body);
    let mut out = Vec::with_capacity(16 + body.len());
    out.extend_from_slice(&state.tag());
    out.extend_from_slice(&body);
    out
}

pub fn crypto_box_open(
    secret: &[u8; 32],
    nonce: &CbNonce,
    packet: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let (tag, cipher) = packet
        .split_first_chunk::<16>()
        .ok_or(CryptoError::CbDecrypt)?;
    let mut state = SbState::from_secret(secret, nonce);
    let mut msg = cipher.to_vec();
    state.decrypt(&mut msg);
    bool::from(state.tag().ct_eq(tag))
        .then_some(msg)
        .ok_or(CryptoError::CbDecrypt)
}

/// `sbEncrypt`: pad to `padded_len`, then box.
pub fn sb_encrypt(
    key: &SbKey,
    nonce: &CbNonce,
    msg: &[u8],
    padded_len: usize,
) -> Result<Vec<u8>, CryptoError> {
    Ok(crypto_box(key.as_bytes(), nonce, &pad(msg, padded_len)?))
}

#[must_use]
pub fn sb_encrypt_no_pad(key: &SbKey, nonce: &CbNonce, msg: &[u8]) -> Vec<u8> {
    crypto_box(key.as_bytes(), nonce, msg)
}

pub fn sb_decrypt(key: &SbKey, nonce: &CbNonce, packet: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let padded = crypto_box_open(key.as_bytes(), nonce, packet)?;
    unpad(&padded).map(<[u8]>::to_vec)
}

pub fn sb_decrypt_no_pad(
    key: &SbKey,
    nonce: &CbNonce,
    packet: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    crypto_box_open(key.as_bytes(), nonce, packet)
}

/// `xorNonce`: xors `prefix` into the start of the nonce.
#[must_use]
pub fn xor_nonce(prefix: &[u8], nonce: &CbNonce) -> CbNonce {
    let mut out = nonce.0;
    out.iter_mut().zip(prefix).for_each(|(n, p)| *n ^= p);
    CbNonce(out)
}
