//! A friend (Types.hs `Contact`): their profile as last received and Zaun's local settings.

use serde::{Deserialize, Serialize};

use crate::engine::agent_port::ConnId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        }
    }

    /// Whether messages can be sent to them.
    #[must_use]
    pub fn reachable(&self) -> bool {
        self.ready && !self.gone
    }
}
