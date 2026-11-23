//! A group connection's agent events, each to the side the member's category says: the host's with its invitee, the joiner's with its host, or a pair introduced to each other (Subscriber.hs `processGroupMessage` agent side).

use super::conn_info::{parse_conn_info, peer_range};
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::{AgentFailure, AgentPort, ConnId};
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{MemberCategory, MemberRecord, MemberStatus};

/// Their profile and range from a handshake (`processMemberProfileUpdate`, `updatePeerChatVRange`).
pub(crate) fn apply_handshake(m: &mut MemberRecord, msg: &ChatMessage, profile: Option<&Profile>) {
    m.chat_v = Some(peer_range(msg));
    if let Some(p) = profile {
        m.profile = p.clone();
    }
}

impl<A: AgentPort> Engine<A> {
    /// `INV`: the link of an invitation the user made for this member.
    pub(crate) fn on_member_invitation(
        &mut self,
        group: i64,
        member: i64,
        _conn: &ConnId,
        full: String,
        _short: Option<String>,
    ) -> Result<()> {
        let mut m = self.member(group, member)?;
        match m.category {
            MemberCategory::Invitee if m.invitation.is_none() => {
                m.invitation = Some(full.clone());
                self.save_member(m)?;
                self.send_group_invitation(group, member, &full)
            }
            MemberCategory::Pre if m.status == MemberStatus::Introduced => {
                self.send_mem_inv(group, member, full)
            }
            MemberCategory::Invitee
            | MemberCategory::Pre
            | MemberCategory::Host
            | MemberCategory::Post => {
                self.log
                    .line(format_args!("a member link arrived again or unasked"));
                Ok(())
            }
        }
    }

    pub(crate) fn on_member_conf(
        &mut self,
        group: i64,
        member: i64,
        conn: &ConnId,
        info: &[u8],
    ) -> Result<()> {
        let msg = parse_conn_info(info)?;
        match self.member(group, member)?.category {
            MemberCategory::Invitee => self.on_invitee_conf(group, member, conn, msg),
            MemberCategory::Pre | MemberCategory::Post => {
                self.on_peer_conf(group, member, conn, msg)
            }
            // A host's CONF is a group link's (`x.grp.link.inv`), handled before it is a member's.
            MemberCategory::Host => {
                self.log
                    .line(format_args!("CONF from a host member dropped"));
                Ok(())
            }
        }
    }

    /// `INFO`: the other side's `x.grp.mem.info`, or a group link host's `x.info`.
    pub(crate) fn on_member_info(
        &mut self,
        group: i64,
        member: i64,
        _conn: &ConnId,
        info: &[u8],
    ) -> Result<()> {
        let msg = parse_conn_info(info)?;
        let mut m = self.member(group, member)?;
        let profile = if let ChatEvent::GrpMemInfo { member_id, profile } = &msg.event {
            if *member_id != m.member_id {
                self.log.line(format_args!(
                    "x.grp.mem.info: memberId is different from expected"
                ));
                return Ok(());
            }
            profile
        } else if let ChatEvent::Info { profile, .. } = &msg.event {
            profile
        } else {
            self.log.line(format_args!(
                "INFO from member must have x.grp.mem.info or x.info"
            ));
            return Ok(());
        };
        let profile = profile.clone();
        apply_handshake(&mut m, &msg, Some(&profile));
        self.save_member(m)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// `CON`; one for a member already connected is a repeat.
    pub(crate) fn on_member_con(&mut self, group: i64, member: i64, _conn: &ConnId) -> Result<()> {
        let m = self.member(group, member)?;
        if m.reachable().is_some() {
            return Ok(());
        }
        match m.category {
            MemberCategory::Host => self.on_host_con(group, m),
            MemberCategory::Invitee => self.on_invitee_con(group, m),
            MemberCategory::Pre | MemberCategory::Post => self.on_peer_con(group, m),
        }
    }

    /// The agent retries what it can; a failure here ends nothing.
    pub(crate) fn on_member_error(
        &mut self,
        _group: i64,
        _member: i64,
        _conn: &ConnId,
        err: &AgentFailure,
    ) -> Result<()> {
        self.log
            .line(format_args!("a group connection failed: {}", err.detail()));
        Ok(())
    }
}
