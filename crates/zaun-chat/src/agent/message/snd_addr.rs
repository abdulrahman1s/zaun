//! A queue named by server and sender ID (Agent/Protocol.hs `SndQAddr`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

use crate::smp::SmpServer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SndQAddr {
    pub server: SmpServer,
    pub sender_id: Vec<u8>,
}

impl Encode for SndQAddr {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.server, &self.sender_id).encode(out)
    }
}

impl Decode for SndQAddr {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (server, sender_id) = <(SmpServer, Vec<u8>)>::decode(d)?;
        Ok(Self { server, sender_id })
    }
}
