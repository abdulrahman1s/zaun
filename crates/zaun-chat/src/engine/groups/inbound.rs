//! A message on a group connection: every element through `on_group_element`, and each event to
//! its handler (Subscriber.hs `processGroupMessage`). One ack follows, as for a friend's.

use crate::chat::{ChatEvent, ChatMessage, parse_chat_elements};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::inbound::asks_receipt;
use crate::engine::machine::Engine;

use super::invitation::Via;

/// Where a group event came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GroupCtx {
    pub group: i64,
    /// The member whose connection it arrived on.
    pub sender: i64,
    /// The member who wrote it: the sender, or the author a forward names.
    pub author: i64,
    pub forwarded: bool,
}

impl GroupCtx {
    #[must_use]
    pub const fn direct(group: i64, sender: i64) -> Self {
        Self {
            group,
            sender,
            author: sender,
            forwarded: false,
        }
    }
}

impl<A: AgentPort> Engine<A> {
    /// Whether a receipt is due.
    pub(crate) fn on_group_body(&mut self, group: i64, sender: i64, body: &[u8]) -> bool {
        let mut receipt = false;
        for element in parse_chat_elements(body) {
            let p = match element {
                Ok(p) => p,
                Err(e) => {
                    self.log
                        .line(format_args!("a group message element was skipped: {e}"));
                    continue;
                }
            };
            receipt |= asks_receipt(&p.message.event);
            if let Err(e) = self.on_group_element(group, sender, p) {
                self.log
                    .line(format_args!("a group event failed: {}", e.detail));
            }
        }
        receipt
    }

    /// The event table. Calls, probes, channel and relay events are not acted on in a group.
    pub(crate) fn on_group_event(&mut self, ctx: GroupCtx, msg: ChatMessage) -> Result<()> {
        let shared = msg.msg_id.map(|m| m.0);
        match msg.event {
            e @ (ChatEvent::MsgNew(_)
            | ChatEvent::MsgFileDescr { .. }
            | ChatEvent::MsgUpdate { .. }
            | ChatEvent::MsgDel { .. }
            | ChatEvent::MsgReact { .. }
            | ChatEvent::File(_)
            | ChatEvent::FileAcpt { .. }
            | ChatEvent::FileAcptInv { .. }
            | ChatEvent::FileCancel { .. }
            | ChatEvent::FileChunk { .. }) => self.on_group_message_event(ctx, shared, e),
            ChatEvent::GrpMsgForward { fwd, msg } => self.on_group_forward_json(ctx, fwd, *msg),
            ChatEvent::GrpInv(inv) => self.on_grp_inv(
                Via::Member {
                    group: ctx.group,
                    member: ctx.sender,
                },
                inv,
            ),
            e @ (ChatEvent::GrpMemNew { .. }
            | ChatEvent::GrpMemIntro { .. }
            | ChatEvent::GrpMemInv { .. }
            | ChatEvent::GrpMemFwd { .. }
            | ChatEvent::GrpMemInfo { .. }
            | ChatEvent::GrpMemCon { .. }
            | ChatEvent::GrpMemConAll { .. }) => self.on_group_membership_event(ctx, e),
            e @ (ChatEvent::GrpLinkMem { .. }
            | ChatEvent::GrpLinkAcpt { .. }
            | ChatEvent::GrpLinkReject(_)) => self.on_group_link_event(ctx, e),
            e @ (ChatEvent::Info { .. }
            | ChatEvent::GrpMemRole { .. }
            | ChatEvent::GrpMemRestrict { .. }
            | ChatEvent::GrpMemDel { .. }
            | ChatEvent::GrpLeave
            | ChatEvent::GrpDel
            | ChatEvent::GrpInfo(_)
            | ChatEvent::GrpPrefs(_)) => self.on_group_admin_event(ctx, e),
            ChatEvent::MsgDeleted
            | ChatEvent::Contact { .. }
            | ChatEvent::Member { .. }
            | ChatEvent::DirectDel
            | ChatEvent::GrpAcpt { .. }
            | ChatEvent::GrpLinkInv(_)
            | ChatEvent::GrpDirectInv { .. }
            | ChatEvent::InfoProbe(_)
            | ChatEvent::InfoProbeCheck(_)
            | ChatEvent::InfoProbeOk(_)
            | ChatEvent::CallInv { .. }
            | ChatEvent::CallOffer { .. }
            | ChatEvent::CallAnswer { .. }
            | ChatEvent::CallExtra { .. }
            | ChatEvent::CallEnd { .. }
            | ChatEvent::Ok
            | ChatEvent::Unknown { .. } => Ok(()),
        }
    }
}
