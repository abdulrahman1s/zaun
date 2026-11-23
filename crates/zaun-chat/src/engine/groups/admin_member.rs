//! A member's role, restriction or removal by an admin (Subscriber.hs `xGrpMemRole`, `xGrpMemRestrict`, `xGrpMemDel`). Roles are checked on the author, never on the member that forwarded.

use zaun_chat_wire::model::ChatNotice;

use super::admin::insufficient;
use super::inbound::GroupCtx;
use super::rank::{at_least, role_required_to_change};
use crate::chat::group::{GroupMemberRole, MemberRestrictions};
use crate::chat::ids::MemberId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::MemberStatus;

impl<A: AgentPort> Engine<A> {
    /// Subscriber.hs 3464 `xGrpMemRole`.
    pub(crate) fn on_mem_role(
        &mut self,
        ctx: GroupCtx,
        member_id: &MemberId,
        role: GroupMemberRole,
    ) -> Result<()> {
        if role == GroupMemberRole::Relay {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.mem.role: relay role can't be assigned",
            ));
        }
        let sender = self.member(ctx.group, ctx.author)?;
        let mut g = self.group(ctx.group)?;
        let target = (*member_id != g.my_member_id)
            .then(|| self.member_by_id(ctx.group, member_id).cloned())
            .map(|m| {
                m.ok_or_else(|| fail(Code::RequestFailed, "x.grp.mem.role with unknown member ID"))
            })
            .transpose()?;
        let from = target.as_ref().map_or(&g.my_role, |m| &m.role);
        if *from == GroupMemberRole::Relay {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.mem.role: relay role can't be changed",
            ));
        }
        if !at_least(&sender.role, &role_required_to_change(from, &role)) {
            return Err(insufficient("x.grp.mem.role"));
        }
        match target {
            Some(mut m) => {
                m.role = role;
                self.save_member(m)?;
            }
            None => {
                g.my_role = role;
                self.save_group(g)?;
            }
        }
        self.state.chats_dirty = true;
        Ok(())
    }

    /// Subscriber.hs 3716 `xGrpMemRestrict`. The user is never restricted by it, and a member's
    /// restriction is not kept (a member's own `blocked` is the user's local choice).
    pub(crate) fn on_mem_restrict(
        &mut self,
        ctx: GroupCtx,
        member_id: &MemberId,
        _restrictions: &MemberRestrictions,
    ) -> Result<()> {
        let g = self.group(ctx.group)?;
        let Some(target) = self.member_by_id(ctx.group, member_id).cloned() else {
            return Ok(());
        };
        if *member_id == g.my_member_id {
            return Ok(());
        }
        let sender = self.member(ctx.group, ctx.author)?;
        let moderator = at_least(&sender.role, &GroupMemberRole::Moderator);
        if !(moderator && at_least(&sender.role, &target.role)) {
            return Err(insufficient("x.grp.mem.restrict"));
        }
        Ok(())
    }

    /// Subscriber.hs 3770 `xGrpMemDel`: the sender must be admin and at least the member's role.
    pub(crate) fn on_mem_del(
        &mut self,
        ctx: GroupCtx,
        member_id: &MemberId,
        messages: bool,
    ) -> Result<()> {
        let sender = self.member(ctx.group, ctx.author)?;
        let g = self.group(ctx.group)?;
        let target = (*member_id != g.my_member_id)
            .then(|| self.member_by_id(ctx.group, member_id).cloned())
            .map(|m| {
                m.ok_or_else(|| fail(Code::RequestFailed, "x.grp.mem.del with unknown member ID"))
            })
            .transpose()?;
        let role = target.as_ref().map_or(&g.my_role, |m| &m.role);
        if !(at_least(&sender.role, &GroupMemberRole::Admin) && at_least(&sender.role, role)) {
            return Err(insufficient("x.grp.mem.del"));
        }
        let by = sender.profile.display_name;
        match target {
            None => self.user_removed(ctx, by, messages),
            Some(m) => self.member_removed(ctx, m.id, by, messages),
        }
    }

    fn user_removed(&mut self, ctx: GroupCtx, by: String, messages: bool) -> Result<()> {
        if self.group(ctx.group)?.status == MemberStatus::Removed {
            return Ok(());
        }
        self.delete_group_link_if_exists(ctx.group)?;
        self.drop_group_conns(ctx.group)?;
        let mut g = self.group(ctx.group)?;
        g.status = MemberStatus::Removed;
        self.save_group(g)?;
        if messages {
            self.mark_member_items_deleted(ctx.group, None)?;
        }
        self.group_notice(ctx.group, Some(ctx.author), ChatNotice::YouRemoved(by))
    }

    /// A member already gone is not told again: the same removal arrives from each forwarder.
    fn member_removed(
        &mut self,
        ctx: GroupCtx,
        member: i64,
        by: String,
        messages: bool,
    ) -> Result<()> {
        self.drop_member_conn(ctx.group, member)?;
        let mut m = self.member(ctx.group, member)?;
        let was_gone = matches!(m.status, MemberStatus::Removed | MemberStatus::Left);
        let who = m.profile.display_name.clone();
        m.status = MemberStatus::Removed;
        self.save_member(m)?;
        if messages {
            self.mark_member_items_deleted(ctx.group, Some(member))?;
        }
        self.state.chats_dirty = true;
        if was_gone {
            return Ok(());
        }
        let notice = ChatNotice::MemberRemoved { by: Some(by), who };
        self.group_notice(ctx.group, Some(ctx.author), notice)
    }
}
