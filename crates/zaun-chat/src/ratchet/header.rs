//! The message header the header key encrypts (Ratchet.hs `MsgHeader`, `legacyMsgHeaderP`).
use super::kem_params::KemParams;
use super::params::require_x448;
use crate::crypto::PublicKey;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Version};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MsgHeader {
    /// The highest ratchet version the sender supports.
    pub max_version: Version,
    pub dhrs: PublicKey,
    pub kem: Option<KemParams>,
    pub pn: u32,
    pub ns: u32,
}

impl MsgHeader {
    /// Headers of 7.0 and older carry no KEM field.
    pub fn decode_with(bytes: &[u8], legacy: bool) -> Result<Self, DecodeError> {
        let mut d = Decoder::new(bytes);
        let max_version = Version::decode(&mut d)?;
        let dhrs = PublicKey::decode(&mut d)?;
        require_x448(&dhrs)?;
        let kem = if legacy {
            None
        } else {
            Option::<KemParams>::decode(&mut d)?
        };
        let (pn, ns) = <(u32, u32)>::decode(&mut d)?;
        Ok(Self {
            max_version,
            dhrs,
            kem,
            pn,
            ns,
        })
    }
}

impl Encode for MsgHeader {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.max_version, &self.dhrs, &self.kem, self.pn, self.ns).encode(out)
    }
}
