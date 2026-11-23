//! Group preferences and their defaults (Types/Preferences.hs `GroupPreferences`, `defaultGroupPrefs`).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureEnabled {
    On,
    Off,
}

/// One group feature: the optional parts are `role` (who may use it), `ttl` (timed messages) and `duration` (comments).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupFeature {
    pub enable: FeatureEnabled,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<i64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl GroupFeature {
    pub fn new(enable: FeatureEnabled) -> Self {
        Self {
            enable,
            role: None,
            ttl: None,
            duration: None,
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timed_messages: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_messages: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_delete: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reactions: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simplex_links: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_messages: Option<GroupFeature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl GroupPreferences {
    /// `defaultGroupPrefs` with every field present.
    pub fn default_full() -> Self {
        use FeatureEnabled::{Off, On};
        let f = |e| Some(GroupFeature::new(e));
        Self {
            timed_messages: Some(GroupFeature {
                ttl: Some(86400),
                ..GroupFeature::new(Off)
            }),
            direct_messages: f(Off),
            full_delete: f(Off),
            reactions: f(On),
            voice: f(On),
            files: f(On),
            simplex_links: f(On),
            reports: f(On),
            history: f(Off),
            support: f(On),
            sessions: f(Off),
            comments: f(Off),
            sign_messages: f(Off),
            commands: Some(Vec::new()),
            extra: Map::new(),
        }
    }
}
