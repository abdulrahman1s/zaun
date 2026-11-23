//! What a group-link host sends over the member connection: its profile, then the verdict on a pending join (Subscriber.hs `xGrpLinkMem`, `xGrpLinkAcpt`; Internal.hs).

use super::inbound::GroupCtx;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::sent_profile;
use crate::engine::records::MemberCategory;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_group_link_event(&mut self, ctx: GroupCtx, e: ChatEvent) -> Result<()> {
        match e {
            ChatEvent::GrpLinkMem { profile, .. } => self.on_link_mem(ctx, &profile),
            ChatEvent::GrpLinkAcpt {
                acceptance,
                role,
                member_id,
            } => self.on_link_acpt(ctx, acceptance, &role, &member_id),
            // The rejection comes in the CONF, never as a message.
            ChatEvent::GrpLinkReject(_) => Ok(()),
            other => Err(fail(
                Code::RequestFailed,
                format!("not a group link event: {other:?}"),
            )),
        }
    }

    /// Only the host the user joined through the link, once (`xGrpLinkMemReceived`; here the
    /// member's `via_link` is cleared by it).
    fn on_link_mem(
        &mut self,
        ctx: GroupCtx,
        profile: &crate::chat::profile::Profile,
    ) -> Result<()> {
        let m = self.member(ctx.group, ctx.sender)?;
        if !(m.via_link && m.category == MemberCategory::Host) {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.link.mem error: invalid group link host profile update",
            ));
        }
        self.update_member_profile(ctx.group, m.id, profile, false)?;
        let mut m = self.member(ctx.group, m.id)?;
        m.via_link = false;
        self.save_member(m).map_err(Into::into)
    }

    /// The joiner's connection with the host works: it gets the host's profile first, then
    /// the members agent introduces it to the rest (Subscriber.hs `sendXGrpLinkMem`).
    pub(crate) fn on_link_member_con(&mut self, group: i64, member: i64) -> Result<bool> {
        let m = self.member(group, member)?;
        let Some(conn) = m.conn.filter(|_| m.via_link) else {
            return Ok(true);
        };
        let msg = ChatMessage::new(ChatEvent::GrpLinkMem {
            profile: sent_profile(&self.state.user),
            member_key: None,
        })?;
        let body = encode_chat_message(&msg, false)?;
        if let Err(e) = self.agent.send(&mut self.store, &conn, body) {
            self.log
                .line(format_args!("x.grp.link.mem not sent: {}", e.detail()));
        }
        Ok(true)
    }
}
