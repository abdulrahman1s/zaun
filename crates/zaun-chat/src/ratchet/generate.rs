//! Fresh handshake params: the private halves kept until the reply, and the public halves sent (Ratchet.hs `generateE2EParams`, `generateRcvE2EParams`, `generateSndE2EParams`, `mkRcvE2ERatchetParams`).
use super::error::{RatchetError, Result};
use super::kem::{KemCiphertext, KemKeyPair, KemSharedSecret, kem_encapsulate, kem_keypair};
use super::kem_params::{KemParams, UseKem};
use super::params::{E2eParams, RcvParams};
use super::pq::PqSupport;
use crate::crypto::{Alg, KeyPair, PrivateKey};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Version};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PrivKem {
    Proposed(KemKeyPair),
    Accepted {
        ct: KemCiphertext,
        shared: KemSharedSecret,
        keys: KemKeyPair,
    },
}

/// Two X448 private keys and the KEM secrets. Stored by the agent between the invitation and the reply, so it has an encoding.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PrivParams {
    pub pk1: PrivateKey,
    pub pk2: PrivateKey,
    pub kem: Option<PrivKem>,
}

fn generate(version: Version, use_kem: Option<&UseKem>) -> Result<(PrivParams, E2eParams)> {
    let (k1, k2) = (KeyPair::generate(Alg::X448)?, KeyPair::generate(Alg::X448)?);
    let kems = use_kem.map(kem_params).transpose()?;
    let (pub_kem, priv_kem) = kems.unzip();
    let params = E2eParams::new(version, k1.public, k2.public, pub_kem)?;
    let private = PrivParams {
        pk1: k1.private,
        pk2: k2.private,
        kem: priv_kem,
    };
    Ok((private, params))
}

fn kem_params(use_kem: &UseKem) -> Result<(KemParams, PrivKem)> {
    let keys = kem_keypair()?;
    Ok(match use_kem {
        UseKem::Propose => (
            KemParams::Proposed(keys.public.clone()),
            PrivKem::Proposed(keys),
        ),
        UseKem::Accept(peer) => {
            let (ct, shared) = kem_encapsulate(peer)?;
            let public = KemParams::Accepted {
                ct: ct.clone(),
                key: keys.public.clone(),
            };
            (public, PrivKem::Accepted { ct, shared, keys })
        }
    })
}

/// Used by the party creating the connection (Bob in the double-ratchet spec).
pub fn generate_rcv_params(version: Version, pq: PqSupport) -> Result<(PrivParams, RcvParams)> {
    let (private, params) = generate(version, pq.0.then_some(&UseKem::Propose))?;
    Ok((private, RcvParams::try_from(params)?))
}

/// Used by the party accepting the connection (Alice); `use_kem` is `reply_kem(..)`.
pub fn generate_snd_params(
    version: Version,
    use_kem: Option<&UseKem>,
) -> Result<(PrivParams, E2eParams)> {
    generate(version, use_kem)
}

impl PrivParams {
    /// `mkRcvE2ERatchetParams`: the public params of a proposal, for a link made again after a restart.
    pub fn rcv_params(&self, version: Version) -> Result<RcvParams> {
        let kem = match &self.kem {
            Some(PrivKem::Proposed(keys)) => Some(KemParams::Proposed(keys.public.clone())),
            Some(PrivKem::Accepted { .. }) => return Err(RatchetError::KemState),
            None => None,
        };
        let params = E2eParams::new(version, self.pk1.public()?, self.pk2.public()?, kem)?;
        RcvParams::try_from(params)
    }
}

impl Encode for PrivKem {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Proposed(keys) => {
                out.push(b'P');
                keys.encode(out)
            }
            Self::Accepted { ct, shared, keys } => {
                out.push(b'A');
                (ct, shared, keys).encode(out)
            }
        }
    }
}

impl Decode for PrivKem {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        match d.byte()? {
            b'P' => Ok(Self::Proposed(KemKeyPair::decode(d)?)),
            b'A' => {
                let (ct, shared, keys) = <_>::decode(d)?;
                Ok(Self::Accepted { ct, shared, keys })
            }
            t => Err(DecodeError::BadTag("private KEM params", t)),
        }
    }
}

impl Encode for PrivParams {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.pk1, &self.pk2, &self.kem).encode(out)
    }
}

impl Decode for PrivParams {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (pk1, pk2, kem) = <_>::decode(d)?;
        Ok(Self { pk1, pk2, kem })
    }
}
