//! What the engine thread keeps in memory: a cache of the records, rebuilt from the store at start.

use std::collections::{BTreeMap, HashMap};

use zaun_chat_wire::model::{ChatMessage, ChatRef};

use super::address::AddressState;
use super::agent_port::ConnId;
use super::calls::CallState;
use super::records::{
    ConnKind, ConnRecord, FriendRecord, GroupRecord, MemberRecord, SettingsRecord, UserRecord,
};

#[derive(Debug, Default)]
pub struct State {
    pub user: UserRecord,
    pub settings: SettingsRecord,
    /// Eviction: a friend leaves with `RemoveFriend`.
    pub friends: BTreeMap<i64, FriendRecord>,
    /// Eviction: with its friend, or when the invitation is evicted (`MAX_INVITATIONS`).
    pub conns: HashMap<ConnId, ConnRecord>,
    /// Eviction: a group leaves when the user deletes it.
    pub groups: BTreeMap<i64, GroupRecord>,
    /// Every group's members other than the user, by (group, member). Eviction: with the group.
    pub members: BTreeMap<(i64, i64), MemberRecord>,
    /// Each conversation's newest item, for the chat list. Eviction: with the friend or group.
    pub last: HashMap<ChatRef, ChatMessage>,
    /// Relay hosts the agent reported and whether each is up. Bounded by the relays in use.
    pub hosts: HashMap<String, bool>,
    pub offline: bool,
    /// The invitation being made: one at a time.
    pub inviting: Option<ConnId>,
    /// The chat list changed and is due out; at most one is pending.
    pub chats_dirty: bool,
    pub address: AddressState,
    pub calls: CallState,
}

impl State {
    #[must_use]
    pub fn friend_of(&self, conn: &ConnId) -> Option<i64> {
        match self.conns.get(conn)?.kind {
            ConnKind::Friend { id } => Some(id),
            ConnKind::Invitation { .. } | ConnKind::Member { .. } | ConnKind::GroupLink { .. } => {
                None
            }
        }
    }

    /// The (group, member) a group connection is with.
    #[must_use]
    pub fn member_of(&self, conn: &ConnId) -> Option<(i64, i64)> {
        match self.conns.get(conn)?.kind {
            ConnKind::Member { group, member } => Some((group, member)),
            ConnKind::Invitation { .. } | ConnKind::Friend { .. } | ConnKind::GroupLink { .. } => {
                None
            }
        }
    }

    /// A group's members other than the user.
    pub fn members_of(&self, group: i64) -> impl Iterator<Item = &MemberRecord> {
        self.members
            .range((group, i64::MIN)..=(group, i64::MAX))
            .map(|(_, m)| m)
    }
}
