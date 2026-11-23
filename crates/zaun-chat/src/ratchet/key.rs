//! A 32-byte secret: ratchet, chain, header and message keys (Crypto.hs `Key`, Ratchet.hs `RatchetKey`).
// TODO(crypto): `crypto::AesKey` hides its bytes, which the ratchet needs to store and compare.
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::crypto::AesKey;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

#[derive(Clone)]
pub struct Key32(Zeroizing<[u8; 32]>);

impl Key32 {
    #[must_use]
    pub fn new(b: [u8; 32]) -> Self {
        Self(Zeroizing::new(b))
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[must_use]
    pub fn aes(&self) -> AesKey {
        AesKey::new(*self.0)
    }
}

impl PartialEq for Key32 {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&*other.0).into()
    }
}

impl Eq for Key32 {}

impl std::fmt::Debug for Key32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key32(..)")
    }
}

/// Raw, no length prefix: only for the stored state.
impl Encode for Key32 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.extend_from_slice(&*self.0);
        Ok(())
    }
}

impl Decode for Key32 {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        d.take_array().map(Self::new)
    }
}
