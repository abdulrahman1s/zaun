//! A received message: every element's `x.*` event handled, then one ack, whatever happened
//! (chat's `withAckMessage`), with a receipt when it held an event that asks for one.

use crate::chat::{ChatEvent, ChatMessage, parse_chat_elements};
use crate::engine::agent_port::{AgentMsgId, AgentPort, ConnId};
use crate::engine::error::Result;
use crate::engine::groups::Via;
use crate::engine::machine::Engine;

/// `hasDeliveryReceipt` (Protocol.hs): only these events are acknowledged with a receipt.
pub(crate) fn asks_receipt(e: &ChatEvent) -> bool {
    matches!(
        e,
        ChatEvent::MsgNew(_) | ChatEvent::GrpInv(_) | ChatEvent::CallInv { .. }
    )
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_msg(&mut self, conn: &ConnId, msg_id: AgentMsgId, body: &[u8]) {
        let friend = self.state.friend_of(conn);
        let receipt = match (friend, self.state.member_of(conn)) {
            (Some(friend), _) => self.on_body(friend, body),
            (None, Some((group, member))) => self.on_group_body(group, member, body),
            (None, None) => {
                self.log
                    .line(format_args!("a message on a connection with no friend"));
                false
            }
        };
        let receipt = (receipt && self.state.settings.send_receipts).then(Vec::new);
        if let Err(e) = self.agent.ack(&mut self.store, conn, msg_id, receipt) {
            self.log.line(format_args!("ack failed: {}", e.detail()));
        }
        let gone = friend.is_some_and(|f| self.state.friends.get(&f).is_some_and(|f| f.gone));
        if gone && let Err(e) = self.forget_conn(conn) {
            self.log
                .line(format_args!("connection not forgotten: {}", e.detail));
        }
    }

    fn on_body(&mut self, friend: i64, body: &[u8]) -> bool {
        let mut receipt = false;
        for element in parse_chat_elements(body) {
            let msg = match element {
                Ok(p) => p.message,
                Err(e) => {
                    self.log
                        .line(format_args!("a message element was skipped: {e}"));
                    continue;
                }
            };
            receipt |= asks_receipt(&msg.event);
            if let Err(e) = self.on_x(friend, msg) {
                self.log
                    .line(format_args!("a chat event failed: {}", e.detail));
            }
        }
        receipt
    }

    fn on_x(&mut self, friend: i64, msg: ChatMessage) -> Result<()> {
        let shared = msg.msg_id.map(|m| m.0);
        match msg.event {
            ChatEvent::MsgNew(c) => self.on_msg_new(friend, shared, c),
            ChatEvent::MsgUpdate {
                msg_id, content, ..
            } => self.on_msg_update(friend, &msg_id.0, &content),
            ChatEvent::MsgDel { msg_id, .. } => self.on_msg_del(friend, &msg_id.0),
            ChatEvent::MsgReact {
                msg_id,
                reaction,
                add,
                ..
            } => self.on_msg_react(friend, &msg_id.0, &reaction, add),
            ChatEvent::Info { profile, .. } => self.on_x_info(friend, &profile),
            ChatEvent::DirectDel => self.on_direct_del(friend),
            e @ (ChatEvent::MsgFileDescr { .. }
            | ChatEvent::File(_)
            | ChatEvent::FileAcpt { .. }
            | ChatEvent::FileAcptInv { .. }
            | ChatEvent::FileCancel { .. }
            | ChatEvent::FileChunk { .. }) => self.on_file_event(friend, shared, e),
            e @ (ChatEvent::CallInv { .. }
            | ChatEvent::CallOffer { .. }
            | ChatEvent::CallAnswer { .. }
            | ChatEvent::CallExtra { .. }
            | ChatEvent::CallEnd { .. }) => self.on_call_event(friend, shared, e),
            ChatEvent::GrpInv(inv) => self.on_grp_inv(Via::Friend(friend), inv),
            // Not carried yet, or never acted on in a direct chat (fact sheet §14): ignored.
            ChatEvent::MsgDeleted
            | ChatEvent::Contact { .. }
            | ChatEvent::Member { .. }
            | ChatEvent::GrpAcpt { .. }
            | ChatEvent::GrpLinkInv(_)
            | ChatEvent::GrpLinkReject(_)
            | ChatEvent::GrpLinkMem { .. }
            | ChatEvent::GrpLinkAcpt { .. }
            | ChatEvent::GrpMemNew { .. }
            | ChatEvent::GrpMemIntro { .. }
            | ChatEvent::GrpMemInv { .. }
            | ChatEvent::GrpMemFwd { .. }
            | ChatEvent::GrpMemInfo { .. }
            | ChatEvent::GrpMemRole { .. }
            | ChatEvent::GrpMemRestrict { .. }
            | ChatEvent::GrpMemCon { .. }
            | ChatEvent::GrpMemConAll { .. }
            | ChatEvent::GrpMemDel { .. }
            | ChatEvent::GrpLeave
            | ChatEvent::GrpDel
            | ChatEvent::GrpInfo(_)
            | ChatEvent::GrpPrefs(_)
            | ChatEvent::GrpDirectInv { .. }
            | ChatEvent::GrpMsgForward { .. }
            | ChatEvent::InfoProbe(_)
            | ChatEvent::InfoProbeCheck(_)
            | ChatEvent::InfoProbeOk(_)
            | ChatEvent::Ok
            | ChatEvent::Unknown { .. } => Ok(()),
        }
    }
}
