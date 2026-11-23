//! Joining by a group link: the host's CONF carries `x.grp.link.inv` (or `.reject`), and the joiner's contact connection becomes the group's, with the host as its first member (Subscriber.hs `saveConnInfo`, Store/Groups.hs `createGroupInvitedViaLink`).

use crate::chat::compress::compress_to_limit;
use crate::chat::group::{GroupAcceptance, MemberIdRole, MemberInfo};
use crate::chat::group_profile::GroupProfile;
use crate::chat::ids::XContactId;
use crate::chat::limits::MAX_CONN_INFO_LENGTH;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage, parse_chat_messages};
use crate::engine::agent_port::{AgentPort, ConnId};
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::sent_profile;
use crate::engine::records::{
    ConnKind, ConnRecord, GroupRecord, MemberCategory, MemberStatus, NS, UserRecord, keys,
};
use zaun_chat_wire::model::ChatRef;

/// What the host's CONF said.
enum Answer {
    Accepted(crate::chat::group::GroupLinkInvitation),
    Rejected(crate::chat::group::GroupLinkRejection),
}

/// The conn-info joining a group link: `x.contact` without a member key, PQ off.
pub(crate) fn group_contact_info(user: &UserRecord, req_id: Vec<u8>) -> Result<Vec<u8>> {
    let msg = ChatMessage::without_id(ChatEvent::Contact {
        profile: sent_profile(user),
        member_key: None,
        contact_req_id: Some(XContactId(req_id)),
        welcome_msg_id: None,
        request_msg: None,
    });
    Ok(compress_to_limit(&msg.to_json()?, MAX_CONN_INFO_LENGTH)?)
}

/// `acceptanceToStatus`.
fn status_of(accepted: GroupAcceptance, profile: &GroupProfile) -> MemberStatus {
    let review = profile
        .member_admission
        .as_ref()
        .and_then(|a| a.review.as_deref())
        == Some("all");
    match accepted {
        GroupAcceptance::PendingApproval => MemberStatus::PendingApproval,
        GroupAcceptance::PendingReview => MemberStatus::PendingReview,
        GroupAcceptance::Accepted if review => MemberStatus::PendingReview,
        GroupAcceptance::Accepted => MemberStatus::Accepted,
    }
}

impl<A: AgentPort> Engine<A> {
    /// The owner's CONF on a contact link the user joined: `Some` when it was a group's link
    /// (`x.grp.link.inv`), which turns the connection into the group's with its host.
    pub(crate) fn on_link_join_conf(&mut self, conn: &ConnId, info: &[u8]) -> Option<Result<()>> {
        let ConnKind::Friend { id } = self.state.conns.get(conn)?.kind else {
            return None;
        };
        let f = self.state.friends.get(&id)?;
        if f.ready || f.link.is_none() {
            return None;
        }
        let answer = parse_chat_messages(info)
            .ok()?
            .into_iter()
            .find_map(|m| match m.event {
                ChatEvent::GrpLinkInv(i) => Some(Answer::Accepted(i)),
                ChatEvent::GrpLinkReject(r) => Some(Answer::Rejected(r)),
                _ => None,
            })?;
        Some(self.become_group(id, conn, answer))
    }

    fn become_group(&mut self, friend: i64, conn: &ConnId, answer: Answer) -> Result<()> {
        let (from, name, key, invited, profile, status) = match answer {
            Answer::Accepted(i) => {
                let status = i
                    .accepted
                    .map_or(MemberStatus::Accepted, |a| status_of(a, &i.group_profile));
                let key = i.from_member_key;
                (
                    i.from_member,
                    i.from_member_name,
                    key,
                    i.invited_member,
                    i.group_profile,
                    status,
                )
            }
            // `nameFromMemberId`: a rejecting host sent no name.
            Answer::Rejected(r) => {
                let name = String::from_utf8_lossy(&r.from_member.member_id.0).into_owned();
                (
                    r.from_member,
                    name,
                    None,
                    r.invited_member,
                    r.group_profile,
                    MemberStatus::Rejected,
                )
            }
        };
        if profile.public_group.is_some() {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.link.inv: publicGroup not allowed in p2p groups",
            ));
        }
        let MemberIdRole {
            member_id,
            member_role,
        } = invited;
        let group = self.store.next_id(keys::GROUP_SEQ)?;
        let mut g = GroupRecord {
            id: group,
            profile,
            my_member_id: member_id,
            my_role: member_role,
            status,
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
        let info = MemberInfo {
            member_id: from.member_id,
            member_role: from.member_role,
            v: None,
            profile: Profile::new(&name, ""),
            member_key: key,
        };
        let mut host = self.new_member(
            group,
            &info,
            MemberCategory::Host,
            MemberStatus::Accepted,
            None,
        )?;
        host.conn = Some(*conn);
        host.via_link = true;
        g.host = Some(host.id);
        self.save_member(host.clone())?;
        self.save_group(g)?;
        self.save_conn(ConnRecord {
            conn: *conn,
            kind: ConnKind::Member {
                group,
                member: host.id,
            },
        })?;
        self.forget_friend_record(friend)?;
        let msg = ChatMessage::without_id(ChatEvent::Info {
            profile: sent_profile(&self.state.user),
            member_key: None,
        });
        let ours = compress_to_limit(&msg.to_json()?, MAX_CONN_INFO_LENGTH)?;
        self.agent.allow(&mut self.store, conn, ours)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// The pending friend a joined link made: its connection lives on as the group's.
    fn forget_friend_record(&mut self, friend: i64) -> Result<()> {
        self.store.delete(NS, &keys::friend(friend))?;
        self.state.friends.remove(&friend);
        self.state.last.remove(&ChatRef::Direct(friend));
        Ok(())
    }
}
