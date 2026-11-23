//! The KEM part of handshake params and of message headers: proposed key, or ciphertext plus next key (Ratchet.hs `RKEMParams`, `UseKEM`, `replyKEM_`).
use super::kem::{KemCiphertext, KemPublicKey};
use super::pq::PqSupport;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

const PROPOSED: u8 = b'P';
const ACCEPTED: u8 = b'A';

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum KemParams {
    /// Only an encapsulation key.
    Proposed(KemPublicKey),
    /// A ciphertext for the peer's key, and our next encapsulation key.
    Accepted {
        ct: KemCiphertext,
        key: KemPublicKey,
    },
}

/// How the party answering a handshake uses the KEM.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UseKem {
    Propose,
    Accept(KemPublicKey),
}

/// `replyKEM_`: accept a proposed key, or propose one, when PQ is supported.
#[must_use]
pub fn reply_kem(proposed: Option<&KemPublicKey>, pq: PqSupport) -> Option<UseKem> {
    if !pq.0 {
        return None;
    }
    Some(proposed.map_or(UseKem::Propose, |k| UseKem::Accept(k.clone())))
}

impl Encode for KemParams {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Proposed(key) => {
                out.push(PROPOSED);
                key.encode(out)
            }
            Self::Accepted { ct, key } => {
                out.push(ACCEPTED);
                ct.encode(out)?;
                key.encode(out)
            }
        }
    }
}

impl Decode for KemParams {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            PROPOSED => Ok(Self::Proposed(KemPublicKey::decode(d)?)),
            ACCEPTED => Ok(Self::Accepted {
                ct: KemCiphertext::decode(d)?,
                key: KemPublicKey::decode(d)?,
            }),
            t => Err(DecodeError::BadTag("ratchet KEM params", t)),
        }
    }
}
