//! Single bytes, Word16, Word32, Int64 (big-endian) and Bool ('T'/'F') (Encoding.hs, encoding.ts).
use super::{
    Decode, Decoder, Encode,
    error::{DecodeError, EncodeError},
};

impl Encode for u8 {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(*self);
        Ok(())
    }
}

impl Decode for u8 {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        d.byte()
    }
}

macro_rules! be_int {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
                out.extend_from_slice(&self.to_be_bytes());
                Ok(())
            }
        }
        impl Decode for $t {
            fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
                Ok(Self::from_be_bytes(d.take_array()?))
            }
        }
    )*};
}

// Int64 is two Word32s, high then low: the same bytes as big-endian two's complement.
be_int!(u16, u32, i64);

impl Encode for bool {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(if *self { b'T' } else { b'F' });
        Ok(())
    }
}

impl Decode for bool {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'T' => Ok(true),
            b'F' => Ok(false),
            t => Err(DecodeError::BadTag("Bool", t)),
        }
    }
}
