//! A host's verdict on a pending join: `x.grp.link.acpt` (Subscriber.hs `xGrpLinkAcpt`).

use zaun_chat_wire::model::ChatNotice;

use super::inbound::GroupCtx;
use super::rank::{at_least, role_required_to_change};
use crate::chat::group::{GroupAcceptance, GroupMemberRole};
use crate::chat::ids::MemberId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::MemberStatus;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_link_acpt(
        &mut self,
        ctx: GroupCtx,
        acceptance: GroupAcceptance,
        role: &GroupMemberRole,
        member_id: &MemberId,
    ) -> Result<()> {
        let sender = self.member(ctx.group, ctx.author)?;
        // Subscriber.hs 2879: the same rule as a role change, a pending member counting as member.
        let needed = role_required_to_change(&GroupMemberRole::Member, role);
        if !at_least(&sender.role, &needed) {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.link.acpt with insufficient member permissions",
            ));
        }
        let g = self.group(ctx.group)?;
        if *member_id == g.my_member_id {
            return self.user_accepted(ctx, acceptance, role);
        }
        let Some(m) = self.member_by_id(ctx.group, member_id).cloned() else {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.link.acpt error: referenced member does not exist",
            ));
        };
        let mut m = m;
        m.role = role.clone();
        m.status = if m.conn.is_some() {
            MemberStatus::Connected
        } else {
            MemberStatus::Announced
        };
        self.save_member(m)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// Subscriber.hs 2905 `processUserAccepted`: only a join that waited for it.
    fn user_accepted(
        &mut self,
        ctx: GroupCtx,
        acceptance: GroupAcceptance,
        role: &GroupMemberRole,
    ) -> Result<()> {
        let mut g = self.group(ctx.group)?;
        if !matches!(
            g.status,
            MemberStatus::PendingApproval | MemberStatus::PendingReview
        ) {
            return Ok(());
        }
        if acceptance == GroupAcceptance::PendingApproval {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.link.acpt: unexpected group acceptance - pending approval",
            ));
        }
        g.my_role = role.clone();
        match acceptance {
            GroupAcceptance::Accepted => {
                g.status = MemberStatus::Connected;
                self.save_group(g)?;
                self.group_notice(ctx.group, Some(ctx.author), ChatNotice::YouJoined)?;
            }
            GroupAcceptance::PendingReview => {
                g.status = MemberStatus::PendingReview;
                self.save_group(g)?;
            }
            GroupAcceptance::PendingApproval => {}
        }
        self.state.chats_dirty = true;
        Ok(())
    }
}
