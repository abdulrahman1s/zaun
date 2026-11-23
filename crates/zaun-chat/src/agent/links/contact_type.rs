//! What a contact short link points to (Agent/Protocol.hs `ContactConnType`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

use crate::agent::error::{AgentError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContactConnType {
    Contact,
    Channel,
    Group,
    Relay,
}

impl ContactConnType {
    /// `ctTypeChar`; short links write it in lower case.
    #[must_use]
    pub fn char(self) -> char {
        match self {
            Self::Contact => 'A',
            Self::Channel => 'C',
            Self::Group => 'G',
            Self::Relay => 'R',
        }
    }

    /// `ctTypeP`.
    pub fn from_char(c: char) -> Result<Self> {
        match c.to_ascii_uppercase() {
            'A' => Ok(Self::Contact),
            'C' => Ok(Self::Channel),
            'G' => Ok(Self::Group),
            'R' => Ok(Self::Relay),
            _ => Err(AgentError::Parse("contact address type")),
        }
    }
}

impl Encode for ContactConnType {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(self.char() as u8);
        Ok(())
    }
}

impl Decode for ContactConnType {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Self::from_char(char::from(d.byte()?))
            .map_err(|_| DecodeError::Invalid("contact address type"))
    }
}
