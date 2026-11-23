//! Contact preferences and the defaults a 7.1.4 user sends (Types/Preferences.hs `Preferences`, `defaultChatPrefs`).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureAllowed {
    Always,
    Yes,
    No,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowPref {
    pub allow: FeatureAllowed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedPref {
    pub allow: FeatureAllowed,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<i64>,
}

/// Unknown preferences are kept in `extra` so a received set round-trips (`_json`, `keepPrefsJSON`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timed_messages: Option<TimedPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_delete: Option<AllowPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reactions: Option<AllowPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<AllowPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<AllowPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<AllowPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<AllowPref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Preferences {
    /// `toChatPrefs defaultChatPrefs`: every field present, `commands` empty.
    pub fn default_sent() -> Self {
        let pref = |allow| Some(AllowPref { allow });
        Self {
            timed_messages: Some(TimedPref {
                allow: FeatureAllowed::Yes,
                ttl: None,
            }),
            full_delete: pref(FeatureAllowed::No),
            reactions: pref(FeatureAllowed::Yes),
            voice: pref(FeatureAllowed::Yes),
            files: pref(FeatureAllowed::Always),
            calls: pref(FeatureAllowed::Yes),
            sessions: pref(FeatureAllowed::No),
            commands: Some(Vec::new()),
            extra: Map::new(),
        }
    }
}
