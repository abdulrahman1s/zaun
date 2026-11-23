//! A friend (Types.hs `Contact`): their profile as last received and Zaun's local settings.

use serde::{Deserialize, Serialize};

use crate::chat::preferences::{FeatureAllowed, Preferences};
use crate::engine::agent_port::ConnId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FriendRecord {
    pub id: i64,
    pub conn: ConnId,
    pub name: String,
    pub about: String,
    /// Their profile's `image`, a `data:` URI as sent.
    pub image: Option<String>,
    pub ready: bool,
    /// They deleted the contact (`x.direct.del`): the conversation stays, nothing more is sent.
    pub gone: bool,
    /// Milliseconds since the epoch.
    pub since: i64,
    pub muted: bool,
    /// Local only (never sent): their messages are deleted on arrival.
    pub blocked: bool,
    /// Items up to this id are read.
    pub read_up_to: i64,
    pub unread: u32,
    /// The link joined, in its `simplex:` form, so pasting it again connects nothing.
    pub link: Option<String>,
    /// The links of our invitation they answered, so ours pasted back connects nothing.
    #[serde(default)]
    pub invited_with: Vec<String>,
    /// Their profile's chat preferences as last received; `None` reads as the defaults.
    #[serde(default)]
    pub prefs: Option<Preferences>,
    /// The security code they were verified with (Haskell `connectionCode`); cleared when it changes.
    #[serde(default)]
    pub verified: Option<String>,
}

/// The chat features whose preferences Zaun enforces or shows (Types/Preferences.hs `ChatFeature`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    FullDelete,
    Reactions,
    Voice,
    Files,
    Calls,
}

/// `PrefEnabled`: whether a feature is on for the user's side and for the friend's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefEnabled {
    pub for_user: bool,
    pub for_contact: bool,
}

impl Feature {
    /// `defaultChatPrefs`, which is also what the user sends: nothing is set per contact.
    const fn default_allow(self) -> FeatureAllowed {
        match self {
            Self::FullDelete => FeatureAllowed::No,
            Self::Reactions | Self::Voice | Self::Calls => FeatureAllowed::Yes,
            Self::Files => FeatureAllowed::Always,
        }
    }

    fn of(self, p: &Preferences) -> Option<FeatureAllowed> {
        let pref = match self {
            Self::FullDelete => &p.full_delete,
            Self::Reactions => &p.reactions,
            Self::Voice => &p.voice,
            Self::Files => &p.files,
            Self::Calls => &p.calls,
        };
        pref.as_ref().map(|a| a.allow)
    }
}

/// `prefEnabled True user contact` (Types/Preferences.hs).
fn pref_enabled(user: FeatureAllowed, contact: FeatureAllowed) -> PrefEnabled {
    use FeatureAllowed::{Always, No};
    let (for_user, for_contact) = match (user, contact) {
        (Always, No) => (false, true),
        (No, Always) => (true, false),
        (_, No) | (No, _) => (false, false),
        _ => (true, true),
    };
    PrefEnabled {
        for_user,
        for_contact,
    }
}

impl FriendRecord {
    #[must_use]
    pub fn new(id: i64, conn: ConnId, since: i64) -> Self {
        Self {
            id,
            conn,
            name: String::new(),
            about: String::new(),
            image: None,
            ready: false,
            gone: false,
            since,
            muted: false,
            blocked: false,
            read_up_to: 0,
            unread: 0,
            link: None,
            invited_with: Vec::new(),
            prefs: None,
            verified: None,
        }
    }

    /// `featureAllowed`'s input: the user's preference against theirs (a missing one is the default).
    #[must_use]
    pub fn enabled(&self, f: Feature) -> PrefEnabled {
        let theirs = self
            .prefs
            .as_ref()
            .and_then(|p| f.of(p))
            .unwrap_or(f.default_allow());
        pref_enabled(f.default_allow(), theirs)
    }

    /// Whether messages can be sent to them.
    #[must_use]
    pub fn reachable(&self) -> bool {
        self.ready && !self.gone
    }
}
