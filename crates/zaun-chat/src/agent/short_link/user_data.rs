//! The opaque user bytes in link data: a ByteString, or `0xFF` and a Large one past 254 bytes (Agent/Protocol.hs `UserLinkData`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserLinkData(pub Vec<u8>);

impl Encode for UserLinkData {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        if self.0.len() <= 254 {
            return self.0.encode(out);
        }
        out.push(0xFF);
        Large(self.0.clone()).encode(out)
    }
}

impl Decode for UserLinkData {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        if d.clone().byte() == Ok(0xFF) {
            d.byte()?;
            return Ok(Self(Large::decode(d)?.0));
        }
        Ok(Self(Vec::<u8>::decode(d)?))
    }
}
