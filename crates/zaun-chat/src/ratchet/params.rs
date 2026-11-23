//! Public E2E ratchet params sent in a handshake: two X448 keys and optional KEM params (Ratchet.hs `E2ERatchetParams`, `RcvE2ERatchetParams`, `SndE2ERatchetParams`).
use super::error::{RatchetError, Result};
use super::kem::KemPublicKey;
use super::kem_params::KemParams;
use crate::crypto::PublicKey;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Version};

/// What the answering party sends back (`SndE2ERatchetParams`): any KEM state.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct E2eParams {
    pub version: Version,
    pub k1: PublicKey,
    pub k2: PublicKey,
    pub kem: Option<KemParams>,
}

/// What the inviting party publishes (`RcvE2ERatchetParams`): the KEM, if any, is only proposed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RcvParams(E2eParams);

pub(crate) fn require_x448(k: &PublicKey) -> Result<(), DecodeError> {
    match k {
        PublicKey::X448(_) => Ok(()),
        PublicKey::Ed25519(_) | PublicKey::Ed448(_) | PublicKey::X25519(_) => {
            Err(DecodeError::Invalid("e2e params: key is not X448"))
        }
    }
}

impl E2eParams {
    pub fn new(
        version: Version,
        k1: PublicKey,
        k2: PublicKey,
        kem: Option<KemParams>,
    ) -> Result<Self> {
        require_x448(&k1)?;
        require_x448(&k2)?;
        Ok(Self {
            version,
            k1,
            k2,
            kem,
        })
    }
}

impl RcvParams {
    #[must_use]
    pub fn params(&self) -> &E2eParams {
        &self.0
    }

    #[must_use]
    pub fn into_params(self) -> E2eParams {
        self.0
    }

    #[must_use]
    pub fn kem_key(&self) -> Option<&KemPublicKey> {
        match &self.0.kem {
            Some(KemParams::Proposed(k)) => Some(k),
            Some(KemParams::Accepted { .. }) | None => None,
        }
    }
}

impl TryFrom<E2eParams> for RcvParams {
    type Error = RatchetError;

    fn try_from(p: E2eParams) -> Result<Self> {
        match p.kem {
            Some(KemParams::Accepted { .. }) => Err(RatchetError::KemState),
            Some(KemParams::Proposed(_)) | None => Ok(Self(p)),
        }
    }
}

impl Encode for E2eParams {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.version, &self.k1, &self.k2, &self.kem).encode(out)
    }
}

impl Decode for E2eParams {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (version, k1, k2, kem) = <_>::decode(d)?;
        require_x448(&k1)?;
        require_x448(&k2)?;
        Ok(Self {
            version,
            k1,
            k2,
            kem,
        })
    }
}

impl Encode for RcvParams {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

impl Decode for RcvParams {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Self::try_from(E2eParams::decode(d)?)
            .map_err(|_| DecodeError::Invalid("bad ratchet KEM state"))
    }
}
