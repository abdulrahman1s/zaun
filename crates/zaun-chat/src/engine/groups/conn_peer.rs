//! Two members the host introduced: CONF with `x.grp.mem.info` answered in kind, then CON and `x.grp.mem.con` to whoever introduced them (Subscriber.hs:851-861, :986-1010).

use zaun_chat_wire::model::ChatNotice;

use super::conn::apply_handshake;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::{AgentPort, ConnId};
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{MemberCategory, MemberRecord, MemberStatus};

impl<A: AgentPort> Engine<A> {
    /// The joiner's side of an introduction: the existing member joined its link.
    pub(crate) fn on_peer_conf(
        &mut self,
        group: i64,
        member: i64,
        conn: &ConnId,
        msg: ChatMessage,
    ) -> Result<()> {
        let ChatEvent::GrpMemInfo { member_id, profile } = &msg.event else {
            self.log
                .line(format_args!("CONF from member must have x.grp.mem.info"));
            return Ok(());
        };
        let mut m = self.member(group, member)?;
        if *member_id != m.member_id {
            self.log.line(format_args!(
                "x.grp.mem.info: memberId is different from expected"
            ));
            return Ok(());
        }
        let info = self.my_mem_info(&self.group(group)?)?;
        self.agent.allow(&mut self.store, conn, info)?;
        apply_handshake(&mut m, &msg, Some(profile));
        self.save_member(m)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    pub(crate) fn on_peer_con(&mut self, group: i64, mut m: MemberRecord) -> Result<()> {
        if !matches!(
            m.status,
            MemberStatus::PendingApproval | MemberStatus::PendingReview
        ) {
            m.status = MemberStatus::Connected;
        }
        let (id, member_id, name) = (m.id, m.member_id.clone(), m.profile.display_name.clone());
        // `sendXGrpMemCon`: a pre-member tells the user's host, a post-member its announcer.
        let tell = match m.category {
            MemberCategory::Pre => self.group(group)?.host,
            MemberCategory::Post => m.invited_by,
            MemberCategory::Host | MemberCategory::Invitee => None,
        };
        self.save_member(m)?;
        self.group_notice(group, Some(id), ChatNotice::MemberJoined(name))?;
        let Some(to) = tell.filter(|t| {
            self.state
                .members
                .get(&(group, *t))
                .and_then(MemberRecord::reachable)
                .is_some()
        }) else {
            return Ok(());
        };
        let msg = ChatMessage::new(ChatEvent::GrpMemCon { member_id })?;
        self.send_to_member(group, to, &msg)?;
        Ok(())
    }
}
