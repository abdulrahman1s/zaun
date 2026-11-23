//! The `Encoding` class split into `Encode` and `Decode` (Encoding.hs `smpEncode`/`smpP`).
use super::{
    Decoder,
    error::{DecodeError, EncodeError},
};

pub trait Encode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError>;

    /// `smpEncode`.
    fn to_bytes(&self) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        self.encode(&mut out)?;
        Ok(out)
    }
}

pub trait Decode: Sized {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError>;

    /// `smpDecode`: the whole input must be consumed.
    fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut d = Decoder::new(bytes);
        let v = Self::decode(&mut d)?;
        d.finish()?;
        Ok(v)
    }
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (**self).encode(out)
    }
}
