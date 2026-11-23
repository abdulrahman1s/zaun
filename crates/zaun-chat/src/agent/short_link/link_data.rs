//! Creating, encrypting and checking link data (Crypto/ShortLink.hs `encodeSignLinkData`, `encryptLinkData`, `decryptLinkData`, AGPL-3.0-only).
use crate::crypto::{PrivateKey, PublicKey, SbKey, sha3_256};
use crate::encoding::{Decode, Encode, VersionRange};

use super::conn_data::{ConnLinkData, UserConnLinkData};
use super::fixed::FixedLinkData;
use super::owner::validate_link_owners;
use super::seal::{decrypt_data, encode_sign, encrypt_data};
use crate::agent::error::{AgentError, Result};
use crate::agent::limits::{FIXED_DATA_PADDED_LENGTH, USER_DATA_PADDED_LENGTH};
use crate::agent::links::{ConnectionMode, ConnectionRequestUri};

/// `QueueLinkData`: what the server stores, each part as `nonce || box`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncLinkData {
    pub fixed: Vec<u8>,
    pub user: Vec<u8>,
}

fn link_err(what: &str) -> AgentError {
    AgentError::Link(what.to_owned())
}

/// `encodeSignFixedData`: the link key (`sha3-256` of the fixed data) and the signed bytes.
pub fn encode_sign_fixed_data(
    root: &PrivateKey,
    agent_v_range: VersionRange,
    conn_req: &ConnectionRequestUri,
    link_entity_id: Option<Vec<u8>>,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let fixed = FixedLinkData {
        agent_v_range,
        root_key: root.public()?,
        link_conn_req: conn_req.binary(),
        link_entity_id,
    };
    let bytes = fixed.to_bytes()?;
    Ok((sha3_256(&bytes).to_vec(), encode_sign(root, &bytes)?))
}

/// `encodeSignUserData`: signed by the root key or by an owner's.
pub fn encode_sign_user_data(
    key: &PrivateKey,
    agent_v_range: VersionRange,
    user: UserConnLinkData,
) -> Result<Vec<u8>> {
    encode_sign(key, &ConnLinkData::new(agent_v_range, user).to_bytes()?)
}

pub fn encrypt_fixed_data(key: &SbKey, signed: &[u8]) -> Result<Vec<u8>> {
    encrypt_data(key, FIXED_DATA_PADDED_LENGTH, signed)
}

pub fn encrypt_user_data(key: &SbKey, signed: &[u8]) -> Result<Vec<u8>> {
    encrypt_data(key, USER_DATA_PADDED_LENGTH, signed)
}

/// `decryptLinkData`: checks the link key against the fixed data, both signatures and the owner chain.
pub fn decrypt_link_data(
    mode: ConnectionMode,
    link_key: &[u8],
    key: &SbKey,
    enc: &EncLinkData,
) -> Result<(FixedLinkData, ConnLinkData)> {
    let (fixed_sig, fixed_bytes) = decrypt_data(key, &enc.fixed)?;
    let (user_sig, user_bytes) = decrypt_data(key, &enc.user)?;
    let parse = |_| link_err("parse link data");
    let fixed = FixedLinkData::from_bytes(&fixed_bytes).map_err(parse)?;
    let link = ConnLinkData::from_bytes(&user_bytes).map_err(parse)?;
    let fixed_mode = match fixed.link_conn_req {
        crate::agent::links::BinaryConnReq::Invitation { .. } => ConnectionMode::Invitation,
        crate::agent::links::BinaryConnReq::Contact { .. } => ConnectionMode::Contact,
    };
    if link.mode() != mode || fixed_mode != mode {
        return Err(link_err("parse link data"));
    }
    if sha3_256(&fixed_bytes).as_slice() != link_key {
        return Err(link_err("link data hash"));
    }
    if !fixed.root_key.verify(&fixed_sig, &fixed_bytes) {
        return Err(link_err("link data signature"));
    }
    let signed_by = |k: &PublicKey| k.verify(&user_sig, &user_bytes);
    let user_ok = match &link {
        ConnLinkData::Invitation { .. } => signed_by(&fixed.root_key),
        ConnLinkData::Contact { data, .. } => {
            validate_link_owners(&fixed.root_key, &data.owners)?;
            signed_by(&fixed.root_key) || data.owners.iter().any(|o| signed_by(&o.owner_key))
        }
    };
    if !user_ok {
        return Err(link_err("user data signature"));
    }
    Ok((fixed, link))
}
