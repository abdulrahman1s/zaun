//! User profile as sent in `x.info` and friends (Types.hs `Profile`, `ChatPeerType`).
use super::image::ImageData;
use super::open_enum::open_enum;
use super::preferences::Preferences;
use serde::{Deserialize, Serialize};
use serde_json::Value;

open_enum!(ChatPeerType { Human = "human", Bot = "bot", Business = "business", Agent = "agent" });

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub display_name: String,
    pub full_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_descr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferences: Option<Preferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_type: Option<ChatPeerType>,
    /// Badge proofs and domain claims are opaque to Zaun; kept for round trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_domain: Option<Value>,
}

impl Profile {
    /// A profile the way a 7.1.4 user sends it: the full default preferences.
    pub fn new(display_name: &str, full_name: &str) -> Self {
        Self {
            display_name: display_name.to_owned(),
            full_name: full_name.to_owned(),
            short_descr: None,
            description: None,
            image: None,
            contact_link: None,
            preferences: Some(Preferences::default_sent()),
            peer_type: None,
            badge: None,
            contact_domain: None,
        }
    }
}
