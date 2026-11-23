//! Being introduced: the joiner makes a link for each member its host introduces (`x.grp.mem.intro`, then `x.grp.mem.inv`), and an existing member joins the link the host forwards (`x.grp.mem.fwd`) (Subscriber.hs `xGrpMemIntro`, `sendXGrpMemInv`, `xGrpMemFwd`).

use super::role::{at_least, may_admit};
use crate::agent::links::ConnectionLink;
use crate::chat::group::{GroupMemberRole, IntroInvitation, MemberInfo};
use crate::chat::version::CHAT_INITIAL_VRANGE;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{ConnKind, ConnRecord, MemberCategory, MemberStatus};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_mem_intro(&mut self, group: i64, sender: i64, info: MemberInfo) -> Result<()> {
        let from = self.member(group, sender)?;
        if from.category != MemberCategory::Host {
            self.log.line(format_args!(
                "x.grp.mem.intro can be only sent by host member"
            ));
            return Ok(());
        }
        // The user's own membership is a member row in 7.1.4, so an intro naming it is a known member.
        let mine = info.member_id == self.group(group)?.my_member_id;
        if mine || self.member_by_id(group, &info.member_id).is_some() {
            self.log.line(format_args!(
                "x.grp.mem.intro ignored: member already exists"
            ));
            return Ok(());
        }
        // Subscriber.hs:3349: the host must still be an admin.
        if !at_least(&from.role, &GroupMemberRole::Admin) {
            self.log.line(format_args!("x.grp.mem.intro: host role"));
            return Ok(());
        }
        if info.v.is_none() {
            self.log.line(format_args!(
                "x.grp.mem.intro: member chat version range incompatible"
            ));
            return Ok(());
        }
        // One invitation connection, PQ off, always (7.1.4 dropped `groupDirectInvVersion`).
        let conn = self
            .agent
            .create_invitation(&mut self.store, false, false, Vec::new())?;
        let mut m = self.new_member(
            group,
            &info,
            MemberCategory::Pre,
            MemberStatus::Introduced,
            None,
        )?;
        m.conn = Some(conn);
        let member = m.id;
        self.save_member(m)?;
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Member { group, member },
        })?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// `sendXGrpMemInv`: the introduced member's link goes to the host to pass on.
    pub(crate) fn send_mem_inv(&mut self, group: i64, member: i64, link: String) -> Result<()> {
        let host = self
            .group(group)?
            .host
            .ok_or_else(|| fail(Code::RequestFailed, "a joined group without a host"))?;
        let mut m = self.member(group, member)?;
        let msg = ChatMessage::new(ChatEvent::GrpMemInv {
            member_id: m.member_id.clone(),
            intro: IntroInvitation {
                group_conn_req: link,
            },
        })?;
        self.send_to_member(group, host, &msg)?;
        m.status = MemberStatus::IntroInvited;
        self.save_member(m)?;
        Ok(())
    }

    /// `xGrpMemFwd`: accepted only from the member's announcer (or about one known only from a
    /// forwarded message); an announcement that has not arrived yet is made here.
    pub(crate) fn on_mem_fwd(
        &mut self,
        group: i64,
        sender: i64,
        info: MemberInfo,
        intro: IntroInvitation,
    ) -> Result<()> {
        let from = self.member(group, sender)?;
        // `checkHostRole` (Subscriber.hs:3381).
        if !may_admit(&from.role, &info.member_role) {
            self.log
                .line(format_args!("x.grp.mem.fwd: forwarding member role"));
            return Ok(());
        }
        let g = self.group(group)?;
        if info.member_id == g.my_member_id {
            self.log
                .line(format_args!("x.grp.mem.fwd: duplicate member ID"));
            return Ok(());
        }
        let mut to = match self.member_by_id(group, &info.member_id).cloned() {
            Some(m) => m,
            None => self.new_member(
                group,
                &info,
                MemberCategory::Post,
                MemberStatus::Announced,
                Some(sender),
            )?,
        };
        let from_announcer = !to.status.gone()
            && (to.invited_by == Some(sender) || to.status == MemberStatus::Unknown);
        if !from_announcer {
            self.log.line(format_args!(
                "x.grp.mem.fwd: not from the member's announcer"
            ));
            return Ok(());
        }
        if to.conn.is_some() {
            self.log.line(format_args!("x.grp.mem.fwd again: dropped"));
            return Ok(());
        }
        let link = ConnectionLink::parse(&intro.group_conn_req)
            .map_err(|e| fail(Code::BadLink, e.to_string()))?;
        let ours = self.my_mem_info(&g)?;
        let conn = self.agent.join(&mut self.store, &link, ours, false)?;
        if !matches!(
            to.status,
            MemberStatus::PendingApproval | MemberStatus::PendingReview
        ) {
            to.status = MemberStatus::IntroInvited;
        }
        to.conn = Some(conn);
        to.chat_v = Some(info.v.unwrap_or(CHAT_INITIAL_VRANGE));
        let member = to.id;
        self.save_member(to)?;
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Member { group, member },
        })?;
        Ok(())
    }
}
