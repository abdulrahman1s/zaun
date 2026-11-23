//! What the engine thread keeps in memory: a cache of the records, rebuilt from the store at start.

use std::collections::{BTreeMap, HashMap};

use zaun_chat_wire::model::ChatMessage;

use super::agent_port::ConnId;
use super::records::{ConnRecord, FriendRecord, SettingsRecord, UserRecord};

#[derive(Debug, Default)]
pub struct State {
    pub user: UserRecord,
    pub settings: SettingsRecord,
    /// Eviction: a friend leaves with `RemoveFriend`.
    pub friends: BTreeMap<i64, FriendRecord>,
    /// Eviction: with its friend, or when the invitation is evicted (`MAX_INVITATIONS`).
    pub conns: HashMap<ConnId, ConnRecord>,
    /// Each friend's newest item, for the chat list. Eviction: with the friend.
    pub last: HashMap<i64, ChatMessage>,
    /// Relay hosts the agent reported and whether each is up. Bounded by the relays in use.
    pub hosts: HashMap<String, bool>,
    pub offline: bool,
    /// The invitation being made: one at a time.
    pub inviting: Option<ConnId>,
    /// The chat list changed and is due out; at most one is pending.
    pub chats_dirty: bool,
}

impl State {
    #[must_use]
    pub fn friend_of(&self, conn: &ConnId) -> Option<i64> {
        match self.conns.get(conn)?.kind {
            super::records::ConnKind::Friend { id } => Some(id),
            super::records::ConnKind::Invitation { .. } => None,
        }
    }
}
