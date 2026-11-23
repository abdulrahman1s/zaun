//! The header in front of every agent message: its sequence number and the hash of the one before (Agent/Protocol.hs `APrivHeader`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct APrivHeader {
    /// Assigned by the sending agent; the confirmation is 1.
    pub snd_msg_id: i64,
    /// `sha256` of the previous message's encoding; empty for the first.
    pub prev_msg_hash: Vec<u8>,
}

impl Encode for APrivHeader {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.snd_msg_id, &self.prev_msg_hash).encode(out)
    }
}

impl Decode for APrivHeader {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (snd_msg_id, prev_msg_hash) = <(i64, Vec<u8>)>::decode(d)?;
        Ok(Self {
            snd_msg_id,
            prev_msg_hash,
        })
    }
}
