//! SystemTime: the seconds as an Int64, nanoseconds dropped (Encoding.hs `instance Encoding SystemTime`).
use super::{
    Decode, Decoder, Encode,
    error::{DecodeError, EncodeError},
};

/// Seconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SysTime(pub i64);

impl Encode for SysTime {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

impl Decode for SysTime {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        i64::decode(d).map(SysTime)
    }
}
