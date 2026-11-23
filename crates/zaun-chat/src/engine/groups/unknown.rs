//! A forward's author not known here becomes a member with status Unknown (Store/Groups.hs `getCreateUnknownGMByMemberId`, `createNewUnknownGroupMember`; Subscriber.hs `unknownMemberRole`).

use crate::chat::group::{GroupMemberRole, MemberInfo};
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{MemberCategory, MemberRecord, MemberStatus};

/// Types.hs `nameFromMemberId`: the first 7 characters of the id's base64.
fn name_from_member_id(id: &MemberId) -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(id.as_bytes());
    b64.chars().take(7).collect()
}

impl<A: AgentPort> Engine<A> {
    /// `getCreateUnknownGMByMemberId`: a member not known here is made, with status Unknown.
    pub(super) fn author_of_forward(
        &mut self,
        group: i64,
        member_id: &MemberId,
        name: &str,
        create: bool,
    ) -> Result<Option<MemberRecord>> {
        if let Some(m) = self.member_by_id(group, member_id) {
            return Ok(Some(m.clone()));
        }
        if !create {
            return Ok(None);
        }
        let name = if name.is_empty() {
            name_from_member_id(member_id)
        } else {
            name.to_owned()
        };
        let info = MemberInfo {
            member_id: member_id.clone(),
            member_role: GroupMemberRole::Author,
            v: None,
            profile: Profile::new(&name, ""),
            member_key: None,
        };
        let m = self.new_member(
            group,
            &info,
            MemberCategory::Pre,
            MemberStatus::Unknown,
            None,
        )?;
        Ok(Some(m))
    }
}
