//! Membership events: `x.grp.mem.new`, `.intro`, `.inv`, `.fwd`, `.con`; `.info` and `.con.all` arrive only as conn-info or not at all (Subscriber.hs `processEvent`, `xGrpMemNew`, `xGrpMemCon`).

use zaun_chat_wire::model::ChatNotice;

use super::inbound::GroupCtx;
use super::role::may_admit;
use crate::chat::ChatEvent;
use crate::chat::container::MsgScope;
use crate::chat::group::MemberInfo;
use crate::chat::ids::MemberId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{MemberCategory, MemberStatus};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_group_membership_event(&mut self, ctx: GroupCtx, e: ChatEvent) -> Result<()> {
        let (group, sender) = (ctx.group, ctx.sender);
        // `xGrpMsgForward` acts on no other membership event (Subscriber.hs:4026).
        if ctx.forwarded && !matches!(e, ChatEvent::GrpMemNew { .. }) {
            self.log.line(format_args!(
                "x.grp.msg.forward: unsupported forwarded event"
            ));
            return Ok(());
        }
        match e {
            ChatEvent::GrpMemNew { member_info, scope } => {
                self.on_mem_new(ctx, &member_info, scope.as_ref())
            }
            ChatEvent::GrpMemIntro { member_info, .. } => {
                self.on_mem_intro(group, sender, member_info)
            }
            ChatEvent::GrpMemInv { member_id, intro } => {
                self.on_mem_inv(group, sender, &member_id, intro)
            }
            ChatEvent::GrpMemFwd { member_info, intro } => {
                self.on_mem_fwd(group, sender, member_info, intro)
            }
            ChatEvent::GrpMemCon { member_id } => self.on_mem_con(group, sender, &member_id),
            // `x.grp.mem.info` is conn-info only, 7.1.4 does not implement `x.grp.mem.con.all`,
            // and the rest never come here (`on_group_event`).
            ChatEvent::GrpMemInfo { .. }
            | ChatEvent::GrpMemConAll { .. }
            | ChatEvent::MsgNew(_)
            | ChatEvent::MsgFileDescr { .. }
            | ChatEvent::MsgUpdate { .. }
            | ChatEvent::MsgDel { .. }
            | ChatEvent::MsgDeleted
            | ChatEvent::MsgReact { .. }
            | ChatEvent::File(_)
            | ChatEvent::FileAcpt { .. }
            | ChatEvent::FileAcptInv { .. }
            | ChatEvent::FileCancel { .. }
            | ChatEvent::Info { .. }
            | ChatEvent::Contact { .. }
            | ChatEvent::Member { .. }
            | ChatEvent::DirectDel
            | ChatEvent::GrpInv(_)
            | ChatEvent::GrpAcpt { .. }
            | ChatEvent::GrpLinkInv(_)
            | ChatEvent::GrpLinkReject(_)
            | ChatEvent::GrpLinkMem { .. }
            | ChatEvent::GrpLinkAcpt { .. }
            | ChatEvent::GrpMemRole { .. }
            | ChatEvent::GrpMemRestrict { .. }
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
            | ChatEvent::CallInv { .. }
            | ChatEvent::CallOffer { .. }
            | ChatEvent::CallAnswer { .. }
            | ChatEvent::CallExtra { .. }
            | ChatEvent::CallEnd { .. }
            | ChatEvent::Ok
            | ChatEvent::FileChunk { .. }
            | ChatEvent::Unknown { .. } => {
                self.log
                    .line(format_args!("unsupported message in a group"));
                Ok(())
            }
        }
    }

    /// `xGrpMemNew`: a member who joined after the user, announced by an admin.
    fn on_mem_new(
        &mut self,
        ctx: GroupCtx,
        info: &MemberInfo,
        scope: Option<&MsgScope>,
    ) -> Result<()> {
        let group = ctx.group;
        let by = self.member(group, ctx.author)?;
        // `checkHostRole` (Subscriber.hs:3246).
        if !may_admit(&by.role, &info.member_role) {
            self.log
                .line(format_args!("x.grp.mem.new: announcing member role"));
            return Ok(());
        }
        if info.member_id == self.group(group)?.my_member_id {
            return Ok(());
        }
        if self
            .member_by_id(group, &info.member_id)
            .is_some_and(|k| k.status != MemberStatus::Unknown)
        {
            self.log
                .line(format_args!("x.grp.mem.new error: member already exists"));
            return Ok(());
        }
        // `initialStatus`: one announced into a member's support scope waits for review.
        let status = match scope {
            Some(MsgScope::Member { .. }) => MemberStatus::PendingReview,
            None => MemberStatus::Announced,
        };
        let mut m = match self.member_by_id(group, &info.member_id).cloned() {
            // `updateUnknownMemberAnnounced`: one known only from a forwarded message.
            Some(mut known) => {
                known.role = info.member_role.clone();
                known.profile = info.profile.clone();
                known.status = status;
                known.chat_v = info.v;
                known.key = known.key.or_else(|| info.member_key.clone());
                known
            }
            None => self.new_member(group, info, MemberCategory::Post, status, Some(by.id))?,
        };
        m.invited_by = Some(by.id);
        let who = m.profile.display_name.clone();
        self.save_member(m)?;
        let notice = ChatNotice::MemberAdded {
            by: Some(by.profile.display_name),
            who,
        };
        self.group_notice(group, Some(by.id), notice)
    }

    /// `xGrpMemCon`: the two members told the host they are connected.
    fn on_mem_con(&mut self, group: i64, sender: i64, member_id: &MemberId) -> Result<()> {
        let Some(other) = self.member_by_id(group, member_id).map(|m| m.id) else {
            self.log.line(format_args!("x.grp.mem.con: no such member"));
            return Ok(());
        };
        self.on_members_connected(group, sender, other)
    }
}
