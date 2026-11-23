//! NewGroup: a group with the user as its creator and owner (Commands.hs `APINewGroup`, `createNewGroupItems`).

use zaun_chat_wire::model::ChatNotice;

use crate::chat::group::GroupMemberRole;
use crate::chat::group_profile::GroupProfile;
use crate::chat::ids::MemberId;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{GroupRecord, MemberStatus, keys};

/// The profile the old glue sent (`{displayName, fullName: ""}`): no preferences, so the group
/// has `defaultGroupPrefs`, history and direct messages off.
fn new_profile(name: &str) -> GroupProfile {
    GroupProfile {
        display_name: name.to_owned(),
        full_name: String::new(),
        short_descr: None,
        description: None,
        image: None,
        public_group: None,
        group_preferences: None,
        member_admission: None,
        chat_layout: None,
    }
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn new_group(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(fail(Code::RequestFailed, "a group needs a name"));
        }
        let id = self.store.next_id(keys::GROUP_SEQ)?;
        let g = GroupRecord {
            id,
            profile: new_profile(name),
            my_member_id: MemberId::mint()?,
            my_role: GroupMemberRole::Owner,
            status: MemberStatus::Creator,
            host: None,
            invitation: None,
            invited_by_friend: None,
            since: self.clock.wall_ms(),
            muted: false,
            read_up_to: 0,
            unread: 0,
            link: None,
            link_conn: None,
        };
        self.save_group(g)?;
        // `CISndGroupE2EEInfo`; the banner and feature items are not shown (codec `notice`).
        self.group_notice(id, None, ChatNotice::GroupCreated)
    }
}
