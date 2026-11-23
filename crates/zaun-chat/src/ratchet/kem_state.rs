//! The ratchet's own KEM state: our key pair, and what we accepted from the peer (Ratchet.hs `RatchetKEM`, `RatchetKEMAccepted`).
use super::kem::{KemCiphertext, KemKeyPair, KemPublicKey, KemSharedSecret};
use super::kem_params::KemParams;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KemAccepted {
    /// The peer's encapsulation key (`rcPQRr`).
    pub peer_key: KemPublicKey,
    pub shared: KemSharedSecret,
    /// What we sent: `encaps(peer_key, shared)`.
    pub ct: KemCiphertext,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RatchetKem {
    pub keys: KemKeyPair,
    pub accepted: Option<KemAccepted>,
}

impl RatchetKem {
    /// What the next headers carry: a proposal, or the accepted ciphertext with our key.
    #[must_use]
    pub fn header_params(&self) -> KemParams {
        match &self.accepted {
            None => KemParams::Proposed(self.keys.public.clone()),
            Some(a) => KemParams::Accepted {
                ct: a.ct.clone(),
                key: self.keys.public.clone(),
            },
        }
    }
}

impl Encode for KemAccepted {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.peer_key, &self.shared, &self.ct).encode(out)
    }
}

impl Decode for KemAccepted {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (peer_key, shared, ct) = <_>::decode(d)?;
        Ok(Self {
            peer_key,
            shared,
            ct,
        })
    }
}

impl Encode for RatchetKem {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.keys, &self.accepted).encode(out)
    }
}

impl Decode for RatchetKem {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (keys, accepted) = <_>::decode(d)?;
        Ok(Self { keys, accepted })
    }
}
