//! Public keys inside links and messages: base64url DER in text, a DER ByteString in binary (Crypto.hs `StrEncoding`/`Encoding` of public keys, AGPL-3.0-only).
use crate::crypto::{Alg, PublicKey};
use crate::encoding::{DecodeError, b64_decode, b64_encode};

use super::error::{AgentError, Result};

/// `strEncode`: padded base64url of the DER.
#[must_use]
pub fn key_str(k: &PublicKey) -> String {
    b64_encode(&k.to_der(), true)
}

pub fn parse_key_str(s: &str, alg: Alg) -> Result<PublicKey> {
    let key = PublicKey::from_der(&b64_decode(s)?).map_err(|_| AgentError::Parse("public key"))?;
    expect_alg(key, alg).map_err(AgentError::from)
}

pub fn expect_alg(key: PublicKey, alg: Alg) -> Result<PublicKey, DecodeError> {
    if key.alg() == alg {
        Ok(key)
    } else {
        Err(DecodeError::Invalid("key algorithm"))
    }
}
