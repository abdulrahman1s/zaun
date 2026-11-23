//! The post-quantum part of ratchet parameters (Crypto/Ratchet.hs `RKEMParams`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large};

/// The post-quantum part: sntrup761 public key (1158 bytes) and ciphertext (1039 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KemParams {
    Proposed { key: Vec<u8> },
    Accepted { ct: Vec<u8>, key: Vec<u8> },
}

impl Encode for KemParams {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Proposed { key } => {
                out.push(b'P');
                Large(key.clone()).encode(out)
            }
            Self::Accepted { ct, key } => {
                out.push(b'A');
                (Large(ct.clone()), Large(key.clone())).encode(out)
            }
        }
    }
}

impl Decode for KemParams {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'P' => Ok(Self::Proposed {
                key: Large::decode(d)?.0,
            }),
            b'A' => {
                let (ct, key) = <(Large, Large)>::decode(d)?;
                Ok(Self::Accepted {
                    ct: ct.0,
                    key: key.0,
                })
            }
            t => Err(DecodeError::BadTag("KEM params", t)),
        }
    }
}
