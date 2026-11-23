//! The wire envelope: encrypted header, message tag and body (Ratchet.hs `EncMessageHeader`, `EncRatchetMessage`, `versionedLengthP`).
use super::lengths::{AUTH_TAG_LEN, IV_LEN};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large, Tail, Version};

/// A length under 32 is the first byte of a 2-byte length; older peers wrote one byte, 32 or more.
fn versioned_length(d: &mut Decoder<'_>) -> Result<(bool, Vec<u8>), DecodeError> {
    if d.clone().byte()? < 32 {
        Ok((false, Large::decode(d)?.0))
    } else {
        Ok((true, Vec::<u8>::decode(d)?))
    }
}

pub struct EncMessageHeader {
    /// The sender's current ratchet version.
    pub version: Version,
    pub iv: [u8; IV_LEN],
    pub tag: [u8; AUTH_TAG_LEN],
    pub body: Vec<u8>,
    pub legacy: bool,
}

impl Encode for EncMessageHeader {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.version.encode(out)?;
        out.extend_from_slice(&self.iv);
        out.extend_from_slice(&self.tag);
        Large(self.body.clone()).encode(out)
    }
}

impl Decode for EncMessageHeader {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let version = Version::decode(d)?;
        let (iv, tag) = (d.take_array()?, d.take_array()?);
        let (legacy, body) = versioned_length(d)?;
        Ok(Self {
            version,
            iv,
            tag,
            body,
            legacy,
        })
    }
}

/// `emHeader` is kept as received: it is part of the message's associated data.
pub struct EncRatchetMessage {
    pub header: Vec<u8>,
    pub tag: [u8; AUTH_TAG_LEN],
    pub body: Vec<u8>,
}

impl Encode for EncRatchetMessage {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        Large(self.header.clone()).encode(out)?;
        out.extend_from_slice(&self.tag);
        out.extend_from_slice(&self.body);
        Ok(())
    }
}

impl Decode for EncRatchetMessage {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (_, header) = versioned_length(d)?;
        let tag = d.take_array()?;
        Ok(Self {
            header,
            tag,
            body: Tail::decode(d)?.0,
        })
    }
}
