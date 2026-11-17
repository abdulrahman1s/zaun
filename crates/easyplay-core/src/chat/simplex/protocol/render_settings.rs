//! Requests that change settings: mute, block, security-code check, the user's profile.
use super::*;

impl Codec {
    pub(super) fn render_set_muted(&mut self, chat: ChatRef, on: bool) -> (RequestKind, String) {
        let mut s = self
            .settings
            .get(&chat)
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({ "favorite": false }));
        let ntfs = match (on, chat) {
            (false, _) => "all",
            (true, ChatRef::Direct(_)) => "none",
            // A muted group still notifies the user's mentions.
            (true, ChatRef::Group(_)) => "mentions",
        };
        s["enableNtfs"] = ntfs.into();
        self.settings.insert(chat, s.clone());
        (
            RequestKind::Mute,
            format!("/_settings {} {s}", chat_ref(chat)),
        )
    }

    /// The engine has no contact block: EasyPlay keeps it in the contact's local custom data.
    pub(super) fn render_set_blocked(&mut self, friend: i64, on: bool) -> (RequestKind, String) {
        let mut data = self
            .custom
            .get(&friend)
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(map) = data.as_object_mut() {
            if on {
                map.insert(CUSTOM_KEY.into(), serde_json::json!({ "blocked": true }));
            } else {
                map.remove(CUSTOM_KEY);
            }
        }
        if on {
            self.blocked.insert(friend);
        } else {
            self.blocked.remove(&friend);
        }
        let empty = data.as_object().is_none_or(serde_json::Map::is_empty);
        self.custom.insert(friend, data.clone());
        let cmd = if empty {
            format!("/_set custom @{friend}")
        } else {
            format!("/_set custom @{friend} {data}")
        };
        (RequestKind::Block, cmd)
    }

    pub(super) fn render_verify(&mut self, friend: i64, code: &str, out: &mut Output) {
        let code: String = code
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == ' ')
            .collect();
        let cmd = format!("/_verify code @{friend} {}", code.trim());
        self.send(out, Pending::Verify(friend), cmd);
    }

    /// The user's engine profile with `p`'s name and line about them; everything else the
    /// engine had (preferences, picture) kept.
    pub(super) fn profile_json(&self, p: &ChatProfile) -> Value {
        let mut v = serde_json::json!({});
        for k in [
            "displayName",
            "fullName",
            "shortDescr",
            "description",
            "image",
            "contactLink",
            "preferences",
            "peerType",
        ] {
            if let Some(x) = self.user_profile.get(k).filter(|x| !x.is_null()) {
                v[k] = x.clone();
            }
        }
        v["displayName"] = p.name.trim().into();
        if v.get("fullName").is_none() {
            v["fullName"] = "".into();
        }
        let about = p.about.trim();
        if about.is_empty() {
            if let Some(m) = v.as_object_mut() {
                m.remove("shortDescr");
            }
        } else {
            v["shortDescr"] = about.into();
        }
        v
    }
}
