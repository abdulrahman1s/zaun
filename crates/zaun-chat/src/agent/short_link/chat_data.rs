//! The JSON the chat layer puts in a link's user data: `{"profile":…,"business":false}`, zstd-compressed past 10240 bytes (simplex-chat Protocol.hs `ContactShortLinkData`, Library/Internal.hs `encodeShortLinkData`, AGPL-3.0-only).
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::user_data::UserLinkData;
use crate::agent::error::{AgentError, Result};
use crate::chat::content::MsgContent;
use crate::chat::limits::MAX_DECOMPRESSED_MSG_LENGTH;
use crate::chat::profile::Profile;
use crate::chat::zstd_frame;

/// Longer JSON is compressed: the link data holds 13784 bytes and the other fields need room.
pub const COMPRESS_ABOVE: usize = 10240;

/// Owner's profile and greeting, in contact addresses and in invitations (where only `profile` is set).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactShortLinkData {
    pub profile: Profile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<MsgContent>,
    #[serde(default)]
    pub business: bool,
    /// `classic`, `terminal` or a name a later version adds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_layout: Option<String>,
    /// Set by the receiving client for its UI; never part of published data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_badge: Option<Value>,
}

impl ContactShortLinkData {
    #[must_use]
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            message: None,
            business: false,
            chat_layout: None,
            local_badge: None,
        }
    }
}

fn chat_err(e: impl std::fmt::Display) -> AgentError {
    AgentError::Link(e.to_string())
}

/// `encodeShortLinkData`.
pub fn encode_short_link_data<T: Serialize>(data: &T) -> Result<UserLinkData> {
    let json = serde_json::to_vec(data).map_err(chat_err)?;
    if json.len() <= COMPRESS_ABOVE {
        return Ok(UserLinkData(json));
    }
    let mut out = vec![b'X'];
    out.extend(zstd_frame::compress(&json).map_err(chat_err)?);
    Ok(UserLinkData(out))
}

/// `decodeLinkUserData`: `None` for empty data.
pub fn decode_short_link_data<T: DeserializeOwned>(data: &UserLinkData) -> Result<Option<T>> {
    let bytes = match data.0.split_first() {
        None => return Ok(None),
        Some((b'X', frame)) => {
            zstd_frame::decompress_limited(frame, MAX_DECOMPRESSED_MSG_LENGTH).map_err(chat_err)?
        }
        Some(_) => data.0.clone(),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(chat_err)
}
