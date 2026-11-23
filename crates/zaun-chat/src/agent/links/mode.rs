//! Whether a connection is a one-time invitation or a long-lived contact address (Agent/Protocol.hs `ConnectionMode`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionMode {
    Invitation,
    Contact,
}

impl Encode for ConnectionMode {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(match self {
            Self::Invitation => b'I',
            Self::Contact => b'C',
        });
        Ok(())
    }
}

impl Decode for ConnectionMode {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'I' => Ok(Self::Invitation),
            b'C' => Ok(Self::Contact),
            t => Err(DecodeError::BadTag("connection mode", t)),
        }
    }
}
