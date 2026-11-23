//! AES-256-GCM with a 16-byte IV (J0 derived by GHASH), as the ratchet message encryption (Crypto.hs `encryptAEAD`, `decryptAEAD`; nook ratchet/crypto.ts).
use aes::Aes256;
use aes_gcm::{
    AesGcm,
    aead::{AeadInOut, KeyInit},
};
use cipher::consts::U16;
use zeroize::Zeroizing;

use super::{CryptoError, pad, unpad};

type Gcm = AesGcm<Aes256, U16>;

pub const AEAD_TAG_LEN: usize = 16;

/// A 32-byte AES key (Crypto.hs `Key`).
#[derive(Clone)]
pub struct AesKey(Zeroizing<[u8; 32]>);

impl AesKey {
    #[must_use]
    pub fn new(key: [u8; 32]) -> Self {
        Self(Zeroizing::new(key))
    }

    pub fn from_slice(b: &[u8]) -> Result<Self, CryptoError> {
        b.try_into()
            .map(Self::new)
            .map_err(|_| CryptoError::BadKey("AES key size"))
    }
}

impl std::fmt::Debug for AesKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AesKey(..)")
    }
}

fn seal(
    key: &AesKey,
    iv: &[u8; 16],
    ad: &[u8],
    buf: &mut [u8],
) -> Result<[u8; AEAD_TAG_LEN], CryptoError> {
    let tag = Gcm::new(&(*key.0).into())
        .encrypt_inout_detached(&(*iv).into(), ad, buf.into())
        .map_err(|_| CryptoError::AesDecrypt)?;
    Ok(tag.into())
}

fn open(
    key: &AesKey,
    iv: &[u8; 16],
    ad: &[u8],
    buf: &mut [u8],
    tag: &[u8; AEAD_TAG_LEN],
) -> Result<(), CryptoError> {
    Gcm::new(&(*key.0).into())
        .decrypt_inout_detached(&(*iv).into(), ad, buf.into(), &(*tag).into())
        .map_err(|_| CryptoError::AesDecrypt)
}

/// `encryptAEAD`: pad to `padded_len`, then the ciphertext and the 16-byte tag apart.
pub fn aead_encrypt(
    key: &AesKey,
    iv: &[u8; 16],
    padded_len: usize,
    ad: &[u8],
    msg: &[u8],
) -> Result<(Vec<u8>, [u8; AEAD_TAG_LEN]), CryptoError> {
    aead_encrypt_no_pad(key, iv, ad, &pad(msg, padded_len)?)
}

pub fn aead_encrypt_no_pad(
    key: &AesKey,
    iv: &[u8; 16],
    ad: &[u8],
    msg: &[u8],
) -> Result<(Vec<u8>, [u8; AEAD_TAG_LEN]), CryptoError> {
    let mut body = msg.to_vec();
    let tag = seal(key, iv, ad, &mut body)?;
    Ok((body, tag))
}

/// `decryptAEAD`: verify, decrypt, unpad.
pub fn aead_decrypt(
    key: &AesKey,
    iv: &[u8; 16],
    ad: &[u8],
    body: &[u8],
    tag: &[u8; AEAD_TAG_LEN],
) -> Result<Vec<u8>, CryptoError> {
    let padded = Zeroizing::new(aead_decrypt_no_pad(key, iv, ad, body, tag)?);
    unpad(&padded)
        .map(<[u8]>::to_vec)
        .map_err(|_| CryptoError::AesDecrypt)
}

pub fn aead_decrypt_no_pad(
    key: &AesKey,
    iv: &[u8; 16],
    ad: &[u8],
    body: &[u8],
    tag: &[u8; AEAD_TAG_LEN],
) -> Result<Vec<u8>, CryptoError> {
    let mut msg = body.to_vec();
    open(key, iv, ad, &mut msg, tag)?;
    Ok(msg)
}
