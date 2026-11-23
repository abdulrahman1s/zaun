//! Group profile (Types.hs `GroupProfile`, `GroupMemberAdmission`).
use super::group_prefs::GroupPreferences;
use super::image::ImageData;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `review: "all"` sends joiners to moderator review first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberAdmission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupProfile {
    pub display_name: String,
    pub full_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_descr: Option<String>,
    /// Used as the welcome message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageData>,
    /// Channels only; opaque.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_group: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_preferences: Option<GroupPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_admission: Option<MemberAdmission>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_layout: Option<String>,
}
