//! Streaming secretbox: any segmentation gives the one-shot result; tag-last helpers for files (Crypto/Lazy.hs `sbInit`, `sbEncryptChunk`, `sbEncryptTailTag`; nook xftp/secretbox.ts).
use cipher::{KeyIvInit, StreamCipher};
use salsa20::XSalsa20;
use subtle::ConstantTimeEq;

use super::{CbNonce, CryptoError, Poly1305, SbKey, box_key, pad_lazy, unpad_lazy};

const TAG_LEN: usize = 16;

/// The first 32 keystream bytes key Poly1305; data continues the same stream, not block aligned.
pub struct SbState {
    cipher: XSalsa20,
    auth: Poly1305,
}

impl SbState {
    #[must_use]
    pub fn new(key: &SbKey, nonce: &CbNonce) -> Self {
        Self::from_secret(key.as_bytes(), nonce)
    }

    pub(super) fn from_secret(secret: &[u8; 32], nonce: &CbNonce) -> Self {
        let mut cipher = XSalsa20::new(&(*box_key(secret)).into(), &(*nonce.as_bytes()).into());
        let mut poly_key = [0u8; 32];
        cipher.apply_keystream(&mut poly_key);
        Self {
            cipher,
            auth: Poly1305::new(&poly_key),
        }
    }

    /// `sbEncryptChunk`, in place.
    pub fn encrypt(&mut self, buf: &mut [u8]) {
        self.cipher.apply_keystream(buf);
        self.auth.update(buf);
    }

    /// `sbDecryptChunk`, in place.
    pub fn decrypt(&mut self, buf: &mut [u8]) {
        self.auth.update(buf);
        self.cipher.apply_keystream(buf);
    }

    /// `sbAuth`: the tag over everything processed.
    #[must_use]
    pub fn tag(self) -> [u8; TAG_LEN] {
        self.auth.finalize()
    }
}

/// `sbEncryptTailTagNoPad`: `ciphertext || tag`.
#[must_use]
pub fn sb_seal_tail_tag_no_pad(key: &SbKey, nonce: &CbNonce, msg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(msg.len() + TAG_LEN);
    out.extend_from_slice(msg);
    let mut state = SbState::new(key, nonce);
    state.encrypt(&mut out);
    out.extend_from_slice(&state.tag());
    out
}

/// `sbDecryptTailTagNoPad` with the validity flag turned into an error.
pub fn sb_open_tail_tag_no_pad(
    key: &SbKey,
    nonce: &CbNonce,
    packet: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let split = packet
        .len()
        .checked_sub(TAG_LEN)
        .ok_or(CryptoError::CbDecrypt)?;
    let (cipher, tag) = packet.split_at(split);
    let mut state = SbState::new(key, nonce);
    let mut msg = cipher.to_vec();
    state.decrypt(&mut msg);
    bool::from(state.tag().ct_eq(tag))
        .then_some(msg)
        .ok_or(CryptoError::CbDecrypt)
}

/// `sbEncryptTailTag`: lazy-pad (8-byte length) to `padded_len`, then `ciphertext || tag`.
pub fn sb_seal_tail_tag(
    key: &SbKey,
    nonce: &CbNonce,
    msg: &[u8],
    padded_len: usize,
) -> Result<Vec<u8>, CryptoError> {
    let padded = pad_lazy(msg, msg.len(), padded_len)?;
    Ok(sb_seal_tail_tag_no_pad(key, nonce, &padded))
}

/// `sbDecryptTailTag`. `padded_len` excludes the tag, as at encryption.
pub fn sb_open_tail_tag(
    key: &SbKey,
    nonce: &CbNonce,
    padded_len: usize,
    packet: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if packet.len() != padded_len + TAG_LEN {
        return Err(CryptoError::CbDecrypt);
    }
    let padded = sb_open_tail_tag_no_pad(key, nonce, packet)?;
    unpad_lazy(&padded).map(<[u8]>::to_vec)
}
