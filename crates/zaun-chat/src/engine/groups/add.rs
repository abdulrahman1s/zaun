//! AddMember: invite a friend with `x.grp.inv` over their contact once the member connection's link is made (Commands.hs `APIAddMember`, `sendGrpInvitation`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatNotice, ChatRef, MessageAuthor, MessageBody};

use super::role::at_least;
use crate::chat::group::{GroupInvitation, GroupMemberRole, MemberIdRole, MemberInfo};
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{
    ConnKind, ConnRecord, FriendRecord, ItemRecord, MemberCategory, MemberStatus, keys,
};

/// Friends' connections are made with PQ keys (`requests/invite.rs`), so their limit is the PQ one.
const FRIEND_PQ: bool = true;

/// The role the page invites with (the old glue's `/_add #g f member`).
const INVITED_ROLE: GroupMemberRole = GroupMemberRole::Member;

pub(crate) fn friend_profile(f: &FriendRecord) -> Profile {
    let mut p = Profile::new(&f.name, "");
    p.short_descr = Some(f.about.clone()).filter(|a| !a.is_empty());
    p.image = f.image.clone().map(crate::chat::image::ImageData);
    p.preferences.clone_from(&f.prefs);
    p
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn add_member(&mut self, group: i64, friend: i64) -> Result<()> {
        let g = self.group(group)?;
        let f = self.friend(friend)?;
        if !f.reachable() {
            return Err(fail(Code::RequestFailed, "the friend is not connected"));
        }
        // `assertUserGroupRole gInfo $ max GRAdmin memRole` (Commands.hs:2848).
        if !at_least(&g.my_role, &GroupMemberRole::Admin) || !g.status.current() {
            return Err(fail(Code::RequestFailed, "only an admin adds members"));
        }
        let existing = self
            .state
            .members_of(group)
            .find(|m| m.friend == Some(friend) && !m.status.gone())
            .cloned();
        match existing {
            None => self.invite_friend(group, &f),
            Some(m) if m.status == MemberStatus::Invited => match m.invitation {
                Some(link) => self.send_group_invitation(group, m.id, &link),
                // The link is still being made; it goes out when it arrives.
                None if m.conn.is_some() => Ok(()),
                None => Err(fail(
                    Code::RequestFailed,
                    "the invitation can't be sent again",
                )),
            },
            Some(_) => Err(fail(Code::RequestFailed, "already a member")),
        }
    }

    fn invite_friend(&mut self, group: i64, f: &FriendRecord) -> Result<()> {
        let info = MemberInfo {
            member_id: MemberId::mint()?,
            member_role: INVITED_ROLE,
            v: None,
            profile: friend_profile(f),
            member_key: None,
        };
        let conn = self
            .agent
            .create_invitation(&mut self.store, false, false, Vec::new())?;
        let mut m = self.new_member(
            group,
            &info,
            MemberCategory::Invitee,
            MemberStatus::Invited,
            None,
        )?;
        m.friend = Some(f.id);
        m.conn = Some(conn);
        let member = m.id;
        self.save_member(m)?;
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Member { group, member },
        })?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// `sendGrpInvitation`: `x.grp.inv` to the friend, and the sent invitation in their chat.
    pub(crate) fn send_group_invitation(
        &mut self,
        group: i64,
        member: i64,
        link: &str,
    ) -> Result<()> {
        let g = self.group(group)?;
        let m = self.member(group, member)?;
        let fid = m
            .friend
            .ok_or_else(|| fail(Code::RequestFailed, "an invitee without a friend"))?;
        let f = self.friend(fid)?;
        let size = self
            .state
            .members_of(group)
            .filter(|m| m.status.current())
            .count()
            + 1;
        let inv = GroupInvitation {
            from_member: MemberIdRole {
                member_id: g.my_member_id.clone(),
                member_role: g.my_role.clone(),
            },
            from_member_key: None,
            invited_member: MemberIdRole {
                member_id: m.member_id.clone(),
                member_role: m.role.clone(),
            },
            conn_request: link.to_owned(),
            group_profile: g.profile.clone(),
            business: None,
            group_link_id: None,
            group_size: i64::try_from(size).ok(),
        };
        let msg = ChatMessage::new(ChatEvent::GrpInv(inv))?;
        let agent_id = self.agent.send(
            &mut self.store,
            &f.conn,
            encode_chat_message(&msg, FRIEND_PQ)?,
        )?;
        let notice = ChatNotice::GroupInvitation {
            group: g.profile.display_name.clone(),
            sent: true,
        };
        let mut item = ItemRecord::new(
            MessageAuthor::Me,
            MessageBody::Notice(notice),
            self.clock.wall_ms(),
        );
        item.shared_id = msg.msg_id.map(|i| i.0);
        let shown = self.add_item(ChatRef::Direct(fid), &item)?;
        self.put_index(&keys::sent(&f.conn, agent_id), shown.id)?;
        self.emit(Event::Messages(vec![shown]));
        self.state.chats_dirty = true;
        Ok(())
    }
}
