//! A group member other than the user (Types.hs `GroupMember`), and its connection.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::group::MemberStatus;
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::chat::version::VersionRange;
use crate::engine::agent_port::ConnId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemberRecord {
    pub group: i64,
    /// Local number, from `keys::MEMBER_SEQ`.
    pub id: i64,
    pub member_id: MemberId,
    pub role: GroupMemberRole,
    pub profile: Profile,
    pub category: MemberCategory,
    pub status: MemberStatus,
    /// The group connection with this member, once one is made or joined.
    pub conn: Option<ConnId>,
    /// The friend this member is, when the user knows them directly.
    pub friend: Option<i64>,
    /// The member who invited or announced this one (`invitedBy`, the forward check's "announcer").
    pub invited_by: Option<i64>,
    /// Local only (never sent): their messages are hidden.
    pub blocked: bool,
    /// Ed25519 member key (X.509 DER, base64url), chat v20; signatures are checked against it.
    pub key: Option<String>,
    /// Their chat version range, once known.
    pub chat_v: Option<VersionRange>,
    /// Joined through the group's link: the host sends `x.grp.link.mem` before introducing it.
    #[serde(default)]
    pub via_link: bool,
    /// Kept by the user as host: how this member stands with each member it was introduced to
    /// or about (Types/MemberRelations.hs); absent means `MRNew`. Eviction: with the member, and
    /// bounded by the group's members.
    #[serde(default)]
    pub relations: BTreeMap<i64, MemberRelation>,
    /// An invitee's invitation link, so a repeated AddMember sends the same one (`getMemberInvitation`).
    #[serde(default)]
    pub invitation: Option<String>,
}

/// `MemberRelation` as far as the host acts on it: introduced and not yet connected, or connected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRelation {
    Introduced,
    Connected,
}

/// `GroupMemberCategory` (Types.hs), the user's relation to the member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberCategory {
    /// Invited by the user.
    Invitee,
    /// Invited the user.
    Host,
    /// Joined before the user; introduced by `x.grp.mem.intro`.
    Pre,
    /// Joined after the user; announced by `x.grp.mem.new`.
    Post,
}

impl MemberRecord {
    /// A connection that carries messages now.
    #[must_use]
    pub fn reachable(&self) -> Option<ConnId> {
        let up = matches!(
            self.status,
            MemberStatus::Connected | MemberStatus::Complete
        );
        self.conn.filter(|_| up)
    }
}
