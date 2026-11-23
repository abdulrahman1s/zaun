//! Someone asks through the group's link: the host makes them a member and accepts with `x.grp.link.inv` (Subscriber.hs `processContactConnMessage` REQ, Internal.hs `acceptGroupJoinRequestAsync`).

use super::rank::assert_user_role;
use crate::chat::compress::compress_to_limit;
use crate::chat::group::{
    GroupAcceptance, GroupLinkInvitation, GroupMemberRole, MemberIdRole, MemberInfo,
};
use crate::chat::ids::MemberId;
use crate::chat::limits::MAX_CONN_INFO_LENGTH;
use crate::chat::profile::Profile;
use crate::chat::version::VersionRange;
use crate::chat::{ChatEvent, ChatMessage, parse_chat_messages};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{ConnKind, ConnRecord, MemberCategory, MemberStatus};

/// The joiner's profile, member key and chat versions from an `x.contact` (or a legacy `x.info`).
fn parse_join(conn_info: &[u8]) -> Option<(Profile, Option<String>, VersionRange)> {
    parse_chat_messages(conn_info)
        .ok()?
        .into_iter()
        .find_map(|m| {
            let v = m.version_range();
            match m.event {
                ChatEvent::Contact {
                    profile,
                    member_key,
                    ..
                }
                | ChatEvent::Info {
                    profile,
                    member_key,
                } => Some((profile, member_key, v)),
                _ => None,
            }
        })
}

impl<A: AgentPort> Engine<A> {
    /// Anything that is not a join request, and a request while the user is no longer admin, is dropped.
    pub(crate) fn on_link_request(
        &mut self,
        group: i64,
        invitation: Vec<u8>,
        conn_info: &[u8],
    ) -> Result<()> {
        let g = self.group(group)?;
        let admin = assert_user_role(&g, &GroupMemberRole::Admin).is_ok();
        let (Some((profile, key, v)), true) = (parse_join(conn_info), admin) else {
            // Subscriber.hs 1650: "ignored join request because host is no longer admin"
            self.log
                .line(format_args!("a group link request was dropped"));
            return self.drop_link_request(&invitation);
        };
        // The link's role is member: `/_create link #1 member` (Commands.hs 3377).
        let role = GroupMemberRole::Member;
        let member_id = MemberId::mint()?;
        let size = self
            .state
            .members_of(group)
            .filter(|m| m.status.current())
            .count();
        let inv = GroupLinkInvitation {
            from_member: MemberIdRole {
                member_id: g.my_member_id.clone(),
                member_role: g.my_role.clone(),
            },
            from_member_name: self.state.user.name.clone(),
            from_member_key: None,
            invited_member: MemberIdRole {
                member_id: member_id.clone(),
                member_role: role.clone(),
            },
            group_profile: g.profile.clone(),
            accepted: Some(GroupAcceptance::Accepted),
            business: None,
            group_size: i64::try_from(size + 1).ok(),
        };
        let msg = ChatMessage::without_id(ChatEvent::GrpLinkInv(inv));
        let info = compress_to_limit(&msg.to_json()?, MAX_CONN_INFO_LENGTH)?;
        // PQ off, like every group connection.
        let conn = self
            .agent
            .accept_request(&mut self.store, &invitation, info, false)?;
        let info = MemberInfo {
            member_id,
            member_role: role,
            v: Some(v),
            profile,
            member_key: key,
        };
        let mut m = self.new_member(
            group,
            &info,
            MemberCategory::Invitee,
            MemberStatus::Accepted,
            None,
        )?;
        m.conn = Some(conn);
        m.via_link = true;
        self.save_member(m.clone())?;
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Member {
                group,
                member: m.id,
            },
        })?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// Forgets a request at the agent; one it no longer has is only logged.
    fn drop_link_request(&mut self, invitation: &[u8]) -> Result<()> {
        if let Err(e) = self.agent.reject_request(&mut self.store, invitation) {
            self.log
                .line(format_args!("request not dropped: {}", e.detail()));
        }
        Ok(())
    }
}
