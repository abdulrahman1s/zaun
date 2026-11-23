//! An admin forwards a member's events to the members it is not connected to yet, as binary `=` batches of `>` elements (Subscriber.hs `isUserGrpFwdRelay`, delivery tasks and `runDeliveryJobWorker`; Messages/Batch.hs `encodeFwdElement`; Protocol.hs `isForwardedGroupMsg`).
//!
//! 7.1.4 sends the same `=` batch to every member whatever its version (`deliver`; the
//! version range `batchDeliveryTasks1` takes is unused), and so does this engine.

use crate::chat::ChatEvent;
use crate::chat::element::{ParsedMsg, encode_forward_element, encode_signed_element};
use crate::chat::forward::{FwdSender, GrpMsgForward};
use crate::chat::group::GroupMemberRole;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::MemberStatus;
use crate::engine::time::iso;

use super::fwd_batch::{forward_batches, fwd_member_name};
use super::policy::at_least;

/// Protocol.hs `isForwardedGroupMsg`: what an admin passes on (a message with an inline file
/// is not). The relay and roster events have no variant here.
#[must_use]
pub(crate) fn forwardable(e: &ChatEvent) -> bool {
    match e {
        ChatEvent::MsgNew(c) => c.file.as_ref().is_none_or(|f| f.file_inline.is_none()),
        ChatEvent::MsgFileDescr { .. }
        | ChatEvent::MsgUpdate { .. }
        | ChatEvent::MsgDel { .. }
        | ChatEvent::MsgReact { .. }
        | ChatEvent::FileCancel { .. }
        | ChatEvent::Info { .. }
        | ChatEvent::GrpMemNew { .. }
        | ChatEvent::GrpMemRole { .. }
        | ChatEvent::GrpMemRestrict { .. }
        | ChatEvent::GrpMemDel { .. }
        | ChatEvent::GrpLeave
        | ChatEvent::GrpDel
        | ChatEvent::GrpInfo(_)
        | ChatEvent::GrpPrefs(_) => true,
        ChatEvent::File(_)
        | ChatEvent::FileAcpt { .. }
        | ChatEvent::FileAcptInv { .. }
        | ChatEvent::FileChunk { .. }
        | ChatEvent::GrpMsgForward { .. }
        | ChatEvent::GrpInv(_)
        | ChatEvent::GrpMemIntro { .. }
        | ChatEvent::GrpMemInv { .. }
        | ChatEvent::GrpMemFwd { .. }
        | ChatEvent::GrpMemInfo { .. }
        | ChatEvent::GrpMemCon { .. }
        | ChatEvent::GrpMemConAll { .. }
        | ChatEvent::GrpLinkMem { .. }
        | ChatEvent::GrpLinkAcpt { .. }
        | ChatEvent::GrpLinkReject(_)
        | ChatEvent::MsgDeleted
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
        | ChatEvent::Unknown { .. } => false,
    }
}

impl<A: AgentPort> Engine<A> {
    /// The `>` element an admin would pass on for this element from `sender`, or `None` when
    /// it is not passed on: the user is no admin, the event is not forwarded, or the sender
    /// still waits for approval. Signed elements keep their signature and bytes; unsigned ones
    /// are encoded again (`verifiedMsgParts`).
    pub(crate) fn relay_element(&self, group: i64, sender: i64, p: &ParsedMsg) -> Option<Vec<u8>> {
        let g = self.state.groups.get(&group)?;
        let m = self.state.members.get(&(group, sender))?;
        let relay = at_least(&g.my_role, &GroupMemberRole::Admin);
        if !relay || m.status == MemberStatus::PendingApproval || !forwardable(&p.message.event) {
            return None;
        }
        let inner = match &p.signed {
            Some(sm) => encode_signed_element(sm).ok()?,
            None => p.message.to_json().ok()?,
        };
        // No broker time reaches the engine; the time it arrived stands in for it.
        let fwd = GrpMsgForward {
            sender: FwdSender::Member {
                member_id: m.member_id.clone(),
                member_name: fwd_member_name(&m.profile.display_name),
            },
            broker_ts: iso(self.clock.wall_ms()),
        };
        encode_forward_element(&fwd, &inner).ok()
    }

    /// To each member `author` was introduced to and is not connected with yet, still in the
    /// group and reachable (`buildMemberList`: `MRIntroduced`, `memberCurrent`). These sends
    /// are no item's, so they are not indexed for delivery.
    pub(crate) fn forward_elements(
        &mut self,
        group: i64,
        author: i64,
        elements: &[Vec<u8>],
    ) -> Result<()> {
        let targets = self.not_yet_connected_to(group, author);
        self.forward_to(group, &targets, elements)
    }

    pub(crate) fn forward_to(
        &mut self,
        group: i64,
        targets: &[i64],
        elements: &[Vec<u8>],
    ) -> Result<()> {
        let (batches, dropped) = forward_batches(elements);
        if dropped > 0 {
            self.log
                .line(format_args!("{dropped} forwarded elements were too large"));
        }
        let conns: Vec<_> = targets
            .iter()
            .filter_map(|t| self.state.members.get(&(group, *t)))
            .filter(|m| m.status.current())
            .filter_map(|m| m.reachable())
            .collect();
        for conn in conns {
            for b in &batches {
                if let Err(e) = self.agent.send(&mut self.store, &conn, b.clone()) {
                    self.log
                        .line(format_args!("a forward was not sent: {}", e.detail()));
                }
            }
        }
        Ok(())
    }
}
