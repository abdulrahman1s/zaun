//! Double-ratchet parameters as they travel in links and envelopes (Crypto/Ratchet.hs `E2ERatchetParams`, `E2ERatchetParamsUri`, `RKEMParams`, AGPL-3.0-only).
// TODO(ratchet): use the types of crate::ratchet once it lands.
use crate::crypto::{Alg, PublicKey};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Version, VersionRange};

use super::kem::KemParams;
use super::keys::expect_alg;

/// `SndE2ERatchetParams` and `RcvE2ERatchetParams`: the KEM part is accepted in the first, proposed in the second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2eParams {
    pub version: Version,
    /// X448.
    pub key1: PublicKey,
    pub key2: PublicKey,
    pub kem: Option<KemParams>,
}

/// The same for links, where the sender offers a range of versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2eParamsUri {
    pub v_range: VersionRange,
    pub key1: PublicKey,
    pub key2: PublicKey,
    pub kem: Option<KemParams>,
}

fn x448(k: PublicKey) -> Result<PublicKey, DecodeError> {
    expect_alg(k, Alg::X448)
}

impl E2eParams {
    /// A receiving side's params (an `AgentRatchetKey`) may only propose a KEM key.
    pub fn check_proposed(self) -> Result<Self, DecodeError> {
        match self.kem {
            Some(KemParams::Accepted { .. }) => Err(DecodeError::Invalid("KEM state")),
            _ => Ok(self),
        }
    }
}

impl Encode for E2eParams {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.version, &self.key1, &self.key2, &self.kem).encode(out)
    }
}

impl Decode for E2eParams {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (version, key1, key2, kem) =
            <(Version, PublicKey, PublicKey, Option<KemParams>)>::decode(d)?;
        Ok(Self {
            version,
            key1: x448(key1)?,
            key2: x448(key2)?,
            kem,
        })
    }
}

impl Encode for E2eParamsUri {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.v_range, &self.key1, &self.key2, &self.kem).encode(out)
    }
}

impl Decode for E2eParamsUri {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (v_range, key1, key2, kem) =
            <(VersionRange, PublicKey, PublicKey, Option<KemParams>)>::decode(d)?;
        Ok(Self {
            v_range,
            key1: x448(key1)?,
            key2: x448(key2)?,
            kem,
        })
    }
}
