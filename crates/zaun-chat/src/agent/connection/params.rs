//! Bridges between the link and envelope forms of ratchet params and the ratchet's own types: both write the same bytes (Crypto/Ratchet.hs `E2ERatchetParams`).
use crate::encoding::{Decode, Encode, Version};
use crate::ratchet::{self, E2eParams, ParamsUri, PrivParams, RcvParams, supported_e2e_range};

use crate::agent::e2e::{E2eParams as WireParams, E2eParamsUri};
use crate::agent::error::Result;

/// The `e2e=` of our invitation link, from the keys we keep until the confirmation.
pub(crate) fn link_params(x3dh: &PrivParams) -> Result<E2eParamsUri> {
    let rcv = x3dh.rcv_params(ratchet::CURRENT_E2E_VERSION)?;
    E2eParamsUri::parse(&ParamsUri::from_params(&rcv, supported_e2e_range()).to_query())
}

/// The inviter's params from its link, at the version both sides speak.
pub(crate) fn rcv_params(uri: &E2eParamsUri, v: Version) -> Result<RcvParams> {
    Ok(ParamsUri::from_query(&uri.to_query())?.to_rcv_params(v)?)
}

pub(crate) fn from_wire(p: &WireParams) -> Result<E2eParams> {
    Ok(E2eParams::from_bytes(&p.to_bytes()?)?)
}
