//! The conn-info of group connections and the member data they carry: `x.grp.acpt`, `x.grp.mem.info`, `MemberInfo` (Internal.hs `encodeXGrpAcpt`, `memberInfo`, `redactedMemberProfile`, `membershipHandshakeProfile`).

use crate::chat::compress::compress_to_limit;
use crate::chat::group::MemberInfo;
use crate::chat::limits::MAX_CONN_INFO_LENGTH;
use crate::chat::profile::Profile;
use crate::chat::version::{CHAT_INITIAL_VRANGE, CURRENT_CHAT_VERSION, VersionRange};
use crate::chat::{ChatEvent, ChatMessage, parse_chat_elements};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::sent_profile;
use crate::engine::records::{GroupRecord, MemberRecord};

/// `redactedMemberProfile` with direct messages off (the default): no preferences, no contact
/// link, no domain claim.
#[must_use]
pub(crate) fn redacted(p: &Profile) -> Profile {
    Profile {
        preferences: None,
        contact_link: None,
        contact_domain: None,
        ..p.clone()
    }
}

/// `memberInfo`: what the host tells others about a member, with the range its connection speaks
/// (a connection's `peerChatVRange` starts at the initial range). 7.1.4 drops an intro without one.
#[must_use]
pub(crate) fn member_info(m: &MemberRecord) -> MemberInfo {
    MemberInfo {
        member_id: m.member_id.clone(),
        member_role: m.role.clone(),
        v: m.conn.map(|_| m.chat_v.unwrap_or(CHAT_INITIAL_VRANGE)),
        profile: redacted(&m.profile),
        member_key: m.key.clone(),
    }
}

/// Group connections are PQ off, so conn-info has the larger limit (`compressConnInfo PQSupportOff`).
pub(crate) fn group_conn_info(event: ChatEvent) -> Result<Vec<u8>> {
    let json = ChatMessage::without_id(event).to_json()?;
    Ok(compress_to_limit(&json, MAX_CONN_INFO_LENGTH)?)
}

/// `parseChatMessage'`: exactly one message, signed or not.
pub(crate) fn parse_conn_info(info: &[u8]) -> Result<ChatMessage> {
    let mut elements = parse_chat_elements(info);
    if elements.len() != 1 {
        return Err(fail(Code::RequestFailed, "conn-info: one message expected"));
    }
    match elements.pop() {
        Some(Ok(p)) => Ok(p.message),
        Some(Err(e)) => Err(e.into()),
        None => Err(fail(Code::RequestFailed, "conn-info: empty")),
    }
}

/// `adjustedMemberVRange`: the peer's range, its top capped at ours.
#[must_use]
pub(crate) fn peer_range(msg: &ChatMessage) -> VersionRange {
    let r = msg.version_range();
    VersionRange::new(r.min, r.max.min(CURRENT_CHAT_VERSION).max(r.min))
}

impl<A: AgentPort> Engine<A> {
    /// `membershipHandshakeProfile`: the user's profile as group members see it.
    pub(crate) fn handshake_profile(&self) -> Profile {
        redacted(&sent_profile(&self.state.user))
    }

    /// `x.grp.mem.info` with the user's member id and profile, the answer on every member connection.
    pub(crate) fn my_mem_info(&self, g: &GroupRecord) -> Result<Vec<u8>> {
        group_conn_info(ChatEvent::GrpMemInfo {
            member_id: g.my_member_id.clone(),
            profile: self.handshake_profile(),
        })
    }
}
