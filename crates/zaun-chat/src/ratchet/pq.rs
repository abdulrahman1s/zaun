//! PQ support (header size, enable-only) and PQ encryption (use the hybrid secret now) flags (Ratchet.hs `PQSupport`, `PQEncryption`, `pqEnableSupport`).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PqSupport(pub bool);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PqEncryption(pub bool);

impl PqSupport {
    pub const ON: Self = Self(true);
    pub const OFF: Self = Self(false);

    #[must_use]
    pub fn to_enc(self) -> PqEncryption {
        PqEncryption(self.0)
    }

    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self(self.0 && other.0)
    }

    /// `pqEnableSupport`: support is only ever turned on.
    #[must_use]
    pub fn enable_with(self, enc: PqEncryption) -> Self {
        Self(self.0 || enc.0)
    }
}

impl PqEncryption {
    pub const ON: Self = Self(true);
    pub const OFF: Self = Self(false);

    #[must_use]
    pub fn to_support(self) -> PqSupport {
        PqSupport(self.0)
    }
}

macro_rules! bool_codec {
    ($t:ident) => {
        impl Encode for $t {
            fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
                self.0.encode(out)
            }
        }

        impl Decode for $t {
            fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
                bool::decode(d).map(Self)
            }
        }
    };
}

bool_codec!(PqSupport);
bool_codec!(PqEncryption);
