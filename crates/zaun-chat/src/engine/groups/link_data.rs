//! What a group link carries: its short-link data and its `clientData` (Types.hs `GroupShortLinkData`, `CRDataGroup`).

use serde::Serialize;

use crate::agent::short_link::encode_short_link_data;
use crate::chat::group_profile::GroupProfile;
use crate::chat::ids::GroupLinkId;
use crate::crypto::random_vec;
use crate::engine::error::{Code, Result, fail};

/// The most a short link's user data may hold (13784 bytes of link data).
const MAX_LINK_DATA: usize = 13784;

/// Types.hs `GroupShortLinkData` without the channel part.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupShortLinkData<'a> {
    group_profile: &'a GroupProfile,
}

/// The profile goes into the link so a joiner sees the group first; a picture that would not fit is left out.
pub(super) fn link_data(profile: &GroupProfile) -> Result<Vec<u8>> {
    let mut profile = profile.clone();
    let mut data = encode_short_link_data(&GroupShortLinkData {
        group_profile: &profile,
    });
    if data.as_ref().map_or(true, |d| d.0.len() > MAX_LINK_DATA) && profile.image.take().is_some() {
        data = encode_short_link_data(&GroupShortLinkData {
            group_profile: &profile,
        });
    }
    data.map(|d| d.0)
        .map_err(|e| fail(Code::RequestFailed, e.to_string()))
}

/// `encodeJSON (CRDataGroup groupLinkId)`.
pub(super) fn client_data() -> Result<String> {
    let id = GroupLinkId(random_vec(16).map_err(|_| fail(Code::RequestFailed, "no randomness"))?);
    serde_json::to_string(&serde_json::json!({ "groupLinkId": id }))
        .map_err(|e| fail(Code::RequestFailed, e.to_string()))
}
