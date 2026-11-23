//! A group seeded straight into the store: the user and members already connected.

use crate::chat::group::{GroupMemberRole, MemberInfo};
use crate::chat::group_profile::GroupProfile;
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::engine::agent_port::ConnId;
use crate::engine::clock::Clock;
use crate::engine::records::{
    ConnKind, ConnRecord, GroupRecord, MemberCategory, MemberStatus, keys,
};
use crate::engine::tests::T;

/// Seeded connections are numbered from here, far from the fake agent's own.
const SEEDED_CONNS: i64 = 1_000_000;

pub fn profile(name: &str) -> GroupProfile {
    GroupProfile {
        display_name: name.into(),
        full_name: String::new(),
        short_descr: None,
        description: None,
        image: None,
        public_group: None,
        group_preferences: None,
        member_admission: None,
        chat_layout: None,
    }
}

impl T {
    /// A group named `name` with the user in `role` and each of `members` connected as a plain
    /// member: the group's number and each member's (number, connection). An owner made it;
    /// otherwise the first member is the host who invited the user.
    pub fn group(
        &mut self,
        name: &str,
        members: &[&str],
        role: GroupMemberRole,
    ) -> (i64, Vec<(i64, ConnId)>) {
        let id = self.e.store.next_id(keys::GROUP_SEQ).unwrap();
        let owner = role == GroupMemberRole::Owner;
        let mut out = Vec::new();
        let mut g = GroupRecord {
            id,
            profile: profile(name),
            my_member_id: MemberId::mint().unwrap(),
            my_role: role,
            status: if owner {
                MemberStatus::Creator
            } else {
                MemberStatus::Connected
            },
            host: None,
            invitation: None,
            invited_by_friend: None,
            since: self.clock.wall_ms(),
            muted: false,
            read_up_to: 0,
            unread: 0,
            link: None,
            link_conn: None,
        };
        for (n, member) in members.iter().enumerate() {
            let host = !owner && n == 0;
            let info = MemberInfo {
                member_id: MemberId::mint().unwrap(),
                member_role: if host {
                    GroupMemberRole::Owner
                } else {
                    GroupMemberRole::Member
                },
                v: None,
                profile: Profile::new(member, ""),
                member_key: None,
            };
            let category = match (owner, host) {
                (true, _) => MemberCategory::Invitee,
                (false, true) => MemberCategory::Host,
                (false, false) => MemberCategory::Pre,
            };
            let mut m = self
                .e
                .new_member(id, &info, category, MemberStatus::Connected, None)
                .unwrap();
            let conn = ConnId(SEEDED_CONNS + m.id);
            m.conn = Some(conn);
            self.e.save_member(m.clone()).unwrap();
            self.e
                .save_conn(ConnRecord {
                    conn,
                    kind: ConnKind::Member {
                        group: id,
                        member: m.id,
                    },
                })
                .unwrap();
            if host {
                g.host = Some(m.id);
            }
            out.push((m.id, conn));
        }
        self.e.save_group(g).unwrap();
        (id, out)
    }
}
