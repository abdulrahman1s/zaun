//! What an agent connection is to the engine: an invitation not answered yet, a friend's, a group member's, or a group link.

use serde::{Deserialize, Serialize};

use crate::engine::agent_port::ConnId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnRecord {
    pub conn: ConnId,
    pub kind: ConnKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ConnKind {
    /// Made by `CreateInvite`; the links arrive with the agent's `Invitation`.
    Invitation {
        full: Option<String>,
        short: Option<String>,
        /// Milliseconds since the epoch, for evicting the oldest.
        created: i64,
    },
    Friend {
        id: i64,
    },
    /// A group connection with a member, from its invitation (made or joined) on. It never
    /// shows as an invitation, takes no part in `inviting`, and is not evicted with them.
    Member {
        group: i64,
        member: i64,
    },
    /// The contact address of a group's link.
    GroupLink {
        group: i64,
    },
}
