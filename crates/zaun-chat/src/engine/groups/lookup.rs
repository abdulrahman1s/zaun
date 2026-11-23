//! Finding a group and its members, and adding a member record.

use crate::chat::group::MemberInfo;
use crate::chat::ids::MemberId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{GroupRecord, MemberCategory, MemberRecord, MemberStatus, keys};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn group(&self, id: i64) -> Result<GroupRecord> {
        self.state
            .groups
            .get(&id)
            .cloned()
            .ok_or_else(|| fail(Code::RequestFailed, "no such group"))
    }

    pub(crate) fn member(&self, group: i64, id: i64) -> Result<MemberRecord> {
        self.state
            .members
            .get(&(group, id))
            .cloned()
            .ok_or_else(|| fail(Code::RequestFailed, "no such member"))
    }

    /// A member by the id the group knows it by.
    pub(crate) fn member_by_id(&self, group: i64, member_id: &MemberId) -> Option<&MemberRecord> {
        self.state
            .members_of(group)
            .find(|m| &m.member_id == member_id)
    }

    /// Stores a new member from its `MemberInfo`, numbered from `keys::MEMBER_SEQ`.
    pub(crate) fn new_member(
        &mut self,
        group: i64,
        info: &MemberInfo,
        category: MemberCategory,
        status: MemberStatus,
        invited_by: Option<i64>,
    ) -> Result<MemberRecord> {
        let id = self.store.next_id(keys::MEMBER_SEQ)?;
        let m = MemberRecord {
            group,
            id,
            member_id: info.member_id.clone(),
            role: info.member_role.clone(),
            profile: info.profile.clone(),
            category,
            status,
            conn: None,
            friend: None,
            invited_by,
            blocked: false,
            key: info.member_key.clone(),
            chat_v: info.v,
            via_link: false,
            relations: std::collections::BTreeMap::new(),
            invitation: None,
        };
        self.save_member(m.clone())?;
        Ok(m)
    }
}
