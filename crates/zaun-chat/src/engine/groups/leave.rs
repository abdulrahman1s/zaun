//! LeaveGroup: `x.grp.leave` to all, then the group connections go (Commands.hs `APILeaveGroup`); and a member's own leaving (Subscriber.hs `xGrpLeave`).

use zaun_chat_wire::model::ChatNotice;

use super::inbound::GroupCtx;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{MemberStatus, NS, keys};
use crate::store::Op;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn leave_group(&mut self, group: i64) -> Result<()> {
        let g = self.group(group)?;
        if g.status.gone() {
            return Ok(());
        }
        self.send_to_group(group, &ChatMessage::new(ChatEvent::GrpLeave)?, None)?;
        self.drop_group_conns(group)?;
        self.delete_group_link_if_exists(group)?;
        let mut g = self.group(group)?;
        g.status = MemberStatus::Left;
        self.save_group(g)?;
        self.group_notice(group, None, ChatNotice::YouLeft)
    }

    /// No role check: anyone may leave. The record stays, as `left`, to keep the story.
    pub(crate) fn on_group_leave(&mut self, ctx: GroupCtx) -> Result<()> {
        let m = self.member(ctx.group, ctx.author)?;
        if m.status == MemberStatus::Left {
            return Ok(());
        }
        self.drop_member_conn(ctx.group, m.id)?;
        let mut m = self.member(ctx.group, m.id)?;
        m.status = MemberStatus::Left;
        let name = m.profile.display_name.clone();
        self.save_member(m)?;
        self.group_notice(ctx.group, Some(ctx.author), ChatNotice::MemberLeft(name))
    }

    /// The agent's connection with the member goes, and the record forgets it
    /// (`deleteMemberConnection`); a member nobody is connected with is left alone.
    pub(crate) fn drop_member_conn(&mut self, group: i64, member: i64) -> Result<()> {
        let mut m = self.member(group, member)?;
        let Some(conn) = m.conn.take() else {
            return Ok(());
        };
        self.forget_conn(&conn)?;
        let ops = self
            .store
            .keys_with_prefix(NS, &keys::sent_prefix(&conn))?
            .into_iter()
            .map(|key| Op::Delete { ns: NS, key })
            .collect();
        self.store.apply(ops)?;
        self.save_member(m)?;
        Ok(())
    }

    /// `deleteGroupConnections`: every member's.
    pub(crate) fn drop_group_conns(&mut self, group: i64) -> Result<()> {
        let with_conn: Vec<i64> = self
            .state
            .members_of(group)
            .filter(|m| m.conn.is_some())
            .map(|m| m.id)
            .collect();
        for member in with_conn {
            self.drop_member_conn(group, member)?;
        }
        Ok(())
    }
}
