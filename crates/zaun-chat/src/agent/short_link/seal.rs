//! Signing and encrypting one piece of link data: `nonce(24) || secretbox(pad(sig || data))` (Crypto/ShortLink.hs `encodeSign`, `encryptData`, AGPL-3.0-only).
use crate::crypto::{CbNonce, PrivateKey, SbKey, Signature, sb_decrypt, sb_encrypt};
use crate::encoding::{Decode, Decoder, Encode, Tail};

use crate::agent::error::{AgentError, Result};

/// `encodeSign`: the signature as a ByteString, then the data.
pub fn encode_sign(key: &PrivateKey, data: &[u8]) -> Result<Vec<u8>> {
    Ok((key.sign(data)?, Tail(data.to_vec())).to_bytes()?)
}

/// `encryptData`: a random nonce, then the box of the padded input.
pub fn encrypt_data(key: &SbKey, padded_len: usize, data: &[u8]) -> Result<Vec<u8>> {
    let nonce = CbNonce::random()?;
    let mut out = nonce.as_bytes().to_vec();
    out.extend(sb_encrypt(key, &nonce, data, padded_len)?);
    Ok(out)
}

/// The signature and the data of one blob, not yet checked.
pub fn decrypt_data(key: &SbKey, blob: &[u8]) -> Result<(Signature, Vec<u8>)> {
    let parse = |_| AgentError::Link("parse link data".to_owned());
    let mut d = Decoder::new(blob);
    let nonce = CbNonce::new(d.take_array().map_err(parse)?);
    let plain = sb_decrypt(key, &nonce, d.take_all())?;
    let (sig, Tail(data)) = <(Signature, Tail)>::from_bytes(&plain).map_err(parse)?;
    Ok((sig, data))
}
