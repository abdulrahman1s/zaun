//! ByteString (1-byte length), `Large` (2-byte length), `Tail` (rest of input), Text/String (Encoding.hs).
use super::{
    Decode, Decoder, Encode,
    error::{DecodeError, EncodeError},
};

/// Raw bytes with no prefix; only valid as the last field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail(pub Vec<u8>);

/// Bytes over 255 with a 2-byte length prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Large(pub Vec<u8>);

fn too_long(what: &'static str, len: usize, max: usize) -> EncodeError {
    EncodeError::TooLong { what, len, max }
}

/// Unlike Haskell, which wraps the length byte, a value over 255 bytes is an error.
impl Encode for [u8] {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let len = u8::try_from(self.len()).map_err(|_| too_long("ByteString", self.len(), 255))?;
        out.push(len);
        out.extend_from_slice(self);
        Ok(())
    }
}

impl Encode for Vec<u8> {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.as_slice().encode(out)
    }
}

impl Decode for Vec<u8> {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let len = usize::from(d.byte()?);
        Ok(d.take(len)?.to_vec())
    }
}

impl<const N: usize> Encode for [u8; N] {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.as_slice().encode(out)
    }
}

impl Encode for str {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.as_bytes().encode(out)
    }
}

impl Encode for String {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.as_str().encode(out)
    }
}

impl Decode for String {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        String::from_utf8(Vec::<u8>::decode(d)?).map_err(|_| DecodeError::Utf8)
    }
}

impl Encode for Large {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let len =
            u16::try_from(self.0.len()).map_err(|_| too_long("Large", self.0.len(), 65535))?;
        len.encode(out)?;
        out.extend_from_slice(&self.0);
        Ok(())
    }
}

impl Decode for Large {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let len = usize::from(u16::decode(d)?);
        Ok(Large(d.take(len)?.to_vec()))
    }
}

impl Encode for Tail {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.extend_from_slice(&self.0);
        Ok(())
    }
}

impl Decode for Tail {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(Tail(d.take_all().to_vec()))
    }
}
