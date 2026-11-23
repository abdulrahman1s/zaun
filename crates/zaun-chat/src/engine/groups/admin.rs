//! Members' profiles and admin events: `x.info`, `x.grp.mem.role`, `.restrict`, `.del`, `x.grp.leave`, `x.grp.del`, `x.grp.info`, `x.grp.prefs`. A check that fails drops the event (Subscriber.hs `xInfoMember`, `xGrpMemRole` … `xGrpPrefs`).

use super::inbound::GroupCtx;
use crate::chat::ChatEvent;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, EngineError, Result, fail};
use crate::engine::machine::Engine;

/// Subscriber.hs `messageError "<event> with insufficient member permissions"`.
pub(super) fn insufficient(event: &str) -> EngineError {
    fail(
        Code::RequestFailed,
        format!("{event} with insufficient member permissions"),
    )
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_group_admin_event(&mut self, ctx: GroupCtx, e: ChatEvent) -> Result<()> {
        match e {
            ChatEvent::Info { profile, .. } => {
                self.update_member_profile(ctx.group, ctx.author, &profile, true)
            }
            ChatEvent::GrpMemRole {
                member_id, role, ..
            } => self.on_mem_role(ctx, &member_id, role),
            ChatEvent::GrpMemRestrict {
                member_id,
                restrictions,
            } => self.on_mem_restrict(ctx, &member_id, &restrictions),
            ChatEvent::GrpMemDel {
                member_id,
                messages,
                ..
            } => self.on_mem_del(ctx, &member_id, messages),
            ChatEvent::GrpLeave => self.on_group_leave(ctx),
            ChatEvent::GrpDel => self.on_group_del(ctx),
            ChatEvent::GrpInfo(p) => self.on_group_info(ctx, p),
            ChatEvent::GrpPrefs(p) => self.on_group_prefs(ctx, p),
            other => Err(fail(
                Code::RequestFailed,
                format!("not an admin event: {other:?}"),
            )),
        }
    }
}
