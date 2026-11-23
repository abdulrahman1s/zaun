//! An invitation to a group (`x.grp.inv`), from a friend or, chat v22, over a member connection (Subscriber.hs `processGroupInvitation`, `xGrpInv`; Store/Groups.hs `createGroupInvitation`).

use zaun_chat_wire::model::ChatNotice;

use super::add::friend_profile;
use super::role::{direct_messages_allowed, may_admit};
use crate::chat::group::{GroupInvitation, MemberInfo};
use crate::chat::profile::Profile;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{GroupRecord, MemberCategory, MemberStatus, keys};

/// The connection an invitation came on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Via {
    Friend(i64),
    Member { group: i64, member: i64 },
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_grp_inv(&mut self, via: Via, inv: GroupInvitation) -> Result<()> {
        let host_profile = match via {
            Via::Friend(id) => friend_profile(&self.friend(id)?),
            Via::Member { group, member } => self.invitation_via_member(group, member)?,
        };
        if inv.group_profile.public_group.is_some() {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.inv: can't invite to channel",
            ));
        }
        // Subscriber.hs:2720 and :3912: the inviter is an admin with at least the role it gives.
        if !may_admit(
            &inv.from_member.member_role,
            &inv.invited_member.member_role,
        ) {
            return Err(fail(Code::RequestFailed, "x.grp.inv: inviting member role"));
        }
        if inv.from_member.member_id == inv.invited_member.member_id {
            return Err(fail(Code::RequestFailed, "x.grp.inv: duplicate member ID"));
        }
        let name = inv.group_profile.display_name.clone();
        self.store_invitation(via, inv, host_profile)?;
        self.invitation_notice(via, name)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// `xGrpInv`'s checks on the member it came from; its profile is the host's.
    fn invitation_via_member(&self, group: i64, member: i64) -> Result<Profile> {
        let via = self.group(group)?;
        let m = self.member(group, member)?;
        if !direct_messages_allowed(&via, &m.role) {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.inv: direct messages not allowed",
            ));
        }
        if m.blocked {
            return Err(fail(Code::RequestFailed, "x.grp.inv: member is blocked"));
        }
        Ok(m.profile)
    }

    /// `createGroupInvitation`: a repeated invitation (the same link) updates the group it made.
    fn store_invitation(&mut self, via: Via, inv: GroupInvitation, host: Profile) -> Result<()> {
        let known = self.state.groups.values().find(|g| {
            g.invitation
                .as_ref()
                .is_some_and(|i| i.conn_request == inv.conn_request)
        });
        if let Some(g) = known.cloned() {
            let mut g = g;
            g.my_member_id = inv.invited_member.member_id.clone();
            g.my_role = inv.invited_member.member_role.clone();
            g.profile = inv.group_profile.clone();
            g.invitation = Some(inv);
            return Ok(self.save_group(g)?);
        }
        let id = self.store.next_id(keys::GROUP_SEQ)?;
        let info = MemberInfo {
            member_id: inv.from_member.member_id.clone(),
            member_role: inv.from_member.member_role.clone(),
            v: None,
            profile: host,
            member_key: inv.from_member_key.clone(),
        };
        let mut h =
            self.new_member(id, &info, MemberCategory::Host, MemberStatus::Invited, None)?;
        h.friend = match via {
            Via::Friend(f) => Some(f),
            Via::Member { group, member } => self.member(group, member)?.friend,
        };
        let host = h.id;
        self.save_member(h)?;
        self.save_group(GroupRecord {
            id,
            profile: inv.group_profile.clone(),
            my_member_id: inv.invited_member.member_id.clone(),
            my_role: inv.invited_member.member_role.clone(),
            status: MemberStatus::Invited,
            host: Some(host),
            invited_by_friend: match via {
                Via::Friend(f) => Some(f),
                Via::Member { .. } => None,
            },
            invitation: Some(inv),
            since: self.clock.wall_ms(),
            muted: false,
            read_up_to: 0,
            unread: 0,
            link: None,
            link_conn: None,
        })?;
        Ok(())
    }

    /// `CIRcvGroupInvitation`: in the friend's chat, or in the group it came through.
    fn invitation_notice(&mut self, via: Via, group: String) -> Result<()> {
        let notice = ChatNotice::GroupInvitation { group, sent: false };
        match via {
            Via::Friend(f) => self.notice(f, notice),
            Via::Member { group, member } => self.group_notice(group, Some(member), notice),
        }
    }
}
