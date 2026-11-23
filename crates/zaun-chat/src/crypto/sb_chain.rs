//! Transport block chain keys (Crypto.hs `sbcInit`, `sbcHkdf`; nook cryptobox.ts).
use zeroize::Zeroizing;

use super::{CbNonce, CryptoError, SbKey, hkdf_sha512};

/// `sbcInit salt secret`: the two 32-byte chain keys (send and receive).
pub fn sbc_init(salt: &[u8], secret: &[u8]) -> Result<(SbKey, SbKey), CryptoError> {
    let out = hkdf_sha512::<64>(salt, secret, b"SimpleXSbChainInit")?;
    Ok((
        SbKey::from_slice(&out[..32])?,
        SbKey::from_slice(&out[32..])?,
    ))
}

/// `sbcHkdf chainKey`: the key and nonce for one block, and the next chain key.
pub fn sbc_hkdf(chain_key: &SbKey) -> Result<(SbKey, CbNonce, SbKey), CryptoError> {
    let out: Zeroizing<[u8; 88]> = hkdf_sha512(&[], chain_key.as_bytes(), b"SimpleXSbChain")?;
    let next = SbKey::from_slice(&out[..32])?;
    let key = SbKey::from_slice(&out[32..64])?;
    let nonce = CbNonce::from_slice(&out[64..])?;
    Ok((key, nonce, next))
}
