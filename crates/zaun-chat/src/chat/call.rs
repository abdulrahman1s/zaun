//! Call signalling payloads (Call.hs `CallInvitation`, `CallOffer`, `WebRTCSession`).
use super::version::VersionRange;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallMedia {
    Audio,
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallCapabilities {
    pub encryption: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallType {
    pub media: CallMedia,
    pub capabilities: CallCapabilities,
}

impl CallType {
    /// `defaultCallType`: encrypted video.
    pub const DEFAULT: Self = Self {
        media: CallMedia::Video,
        capabilities: CallCapabilities { encryption: true },
    };
}

/// `call_dh_pub_key` is the padded base64url X.509 X25519 key; `call_v_range` is `"1-2"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallInvitation {
    pub call_type: CallType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_dh_pub_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_v_range: Option<VersionRange>,
}

/// The strings are LZ-compressed JSON that the core never looks into.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRtcSession {
    pub rtc_session: String,
    pub rtc_ice_candidates: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallOffer {
    pub call_type: CallType,
    pub rtc_session: WebRtcSession,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_dh_pub_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_version: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallAnswer {
    pub rtc_session: WebRtcSession,
}

/// `rtc_session` carries an ICE-restart offer or answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRtcExtraInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtc_session: Option<String>,
    pub rtc_ice_candidates: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallExtraInfo {
    pub rtc_extra_info: WebRtcExtraInfo,
}
