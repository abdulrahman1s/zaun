//! A forwarded message (a `>` element or the JSON `x.grp.msg.forward` of older hosts), taken from its author (Subscriber.hs `xGrpMsgForward`, `processForwardedMsg`; Store/Groups.hs `getCreateUnknownGMByMemberId`).

use crate::chat::forward::{FwdSender, GrpMsgForward};
use crate::chat::group::GroupMemberRole;
use crate::chat::signed::SignedMsg;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{MemberCategory, MemberStatus};

use super::inbound::GroupCtx;
use super::policy::at_least;
use super::relay::forwardable;

fn dropped(why: &str) -> crate::engine::error::EngineError {
    fail(Code::RequestFailed, why.to_owned())
}

/// The forward header's broker time in milliseconds; the binary form has whole seconds.
fn forward_time(fwd: &GrpMsgForward) -> Option<i64> {
    let bytes = fwd.encode().ok()?;
    let secs = bytes.rchunks(8).next()?;
    Some(i64::from_be_bytes(secs.try_into().ok()?).saturating_mul(1000))
}

impl<A: AgentPort> Engine<A> {
    /// The JSON `x.grp.msg.forward` of older hosts. Forwards nest at most once (`maxFwdDepth`).
    pub(crate) fn on_group_forward_json(
        &mut self,
        ctx: GroupCtx,
        fwd: GrpMsgForward,
        msg: ChatMessage,
    ) -> Result<()> {
        if ctx.forwarded {
            return Err(dropped("x.grp.msg.forward: nested forward"));
        }
        self.on_forwarded(ctx.group, ctx.sender, &fwd, None, msg)
    }

    /// Subscriber.hs:4026-4055, in order: the forwarder is an admin; the author is not the
    /// user, not removed, and one this forwarder may pass on for. A forward from a channel
    /// (`FwdChannel`) is out of scope here.
    pub(crate) fn on_forwarded(
        &mut self,
        group: i64,
        forwarder: i64,
        fwd: &GrpMsgForward,
        signed: Option<&SignedMsg>,
        msg: ChatMessage,
    ) -> Result<()> {
        let by = self.member(group, forwarder)?;
        if !at_least(&by.role, &GroupMemberRole::Admin) {
            return Err(dropped("x.grp.msg.forward: the forwarder is not an admin"));
        }
        let FwdSender::Member {
            member_id,
            member_name,
        } = &fwd.sender
        else {
            return Err(dropped(
                "x.grp.msg.forward: channel forwards are not carried",
            ));
        };
        if *member_id == self.group(group)?.my_member_id {
            return Err(dropped(
                "x.grp.msg.forward: content attributed to own membership",
            ));
        }
        let leave = matches!(msg.event, ChatEvent::GrpLeave);
        let Some(author) = self.author_of_forward(group, member_id, member_name, !leave)? else {
            return Ok(());
        };
        if author.status.gone() {
            self.log.line(format_args!(
                "x.grp.msg.forward: content from a removed member"
            ));
            return Ok(());
        }
        // `expectedForwarder`: the user's host, the author's inviter, or the author is unknown.
        let expected = by.category == MemberCategory::Host
            || author.invited_by == Some(by.id)
            || author.status == MemberStatus::Unknown;
        if !expected {
            return Err(dropped("x.grp.msg.forward: unexpected forwarder"));
        }
        self.check_signature(group, author.id, signed)?;
        let ctx = GroupCtx {
            group,
            sender: forwarder,
            author: author.id,
            forwarded: true,
        };
        self.forwarded_event(ctx, fwd, msg)
    }

    /// `processForwardedMsg`: only the events an admin forwards are taken; content takes the
    /// forward's time.
    fn forwarded_event(
        &mut self,
        ctx: GroupCtx,
        fwd: &GrpMsgForward,
        msg: ChatMessage,
    ) -> Result<()> {
        if !forwardable(&msg.event) {
            return Err(dropped("x.grp.msg.forward: unsupported forwarded event"));
        }
        let content = matches!(
            msg.event,
            ChatEvent::MsgNew(_)
                | ChatEvent::MsgFileDescr { .. }
                | ChatEvent::MsgUpdate { .. }
                | ChatEvent::MsgDel { .. }
                | ChatEvent::MsgReact { .. }
                | ChatEvent::FileCancel { .. }
        );
        if !content {
            return self.on_group_event(ctx, msg);
        }
        let at = forward_time(fwd).unwrap_or_else(|| self.clock.wall_ms());
        let shared = msg.msg_id.map(|m| m.0);
        self.on_group_message_at(ctx, shared, msg.event, at)
    }
}
