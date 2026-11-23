//! A group and its members as the page sees them (codec `parse_chats`, `parse_members`).

use zaun_chat_wire::model::{ChatGroup, GroupMember, Membership};

use crate::chat::group::GroupMemberRole;
use crate::engine::agent_port::AgentPort;
use crate::engine::machine::Engine;
use crate::engine::profile::picture_of;
use crate::engine::records::{GroupRecord, MemberRecord, MemberStatus};

/// Codec `membership`.
pub(crate) const fn membership(status: MemberStatus) -> Membership {
    match status {
        MemberStatus::Invited => Membership::Invited,
        MemberStatus::Accepted | MemberStatus::PendingApproval | MemberStatus::PendingReview => {
            Membership::Joining
        }
        MemberStatus::Left
        | MemberStatus::Removed
        | MemberStatus::GroupDeleted
        | MemberStatus::Rejected => Membership::Left,
        MemberStatus::Unknown
        | MemberStatus::Introduced
        | MemberStatus::IntroInvited
        | MemberStatus::Announced
        | MemberStatus::Connected
        | MemberStatus::Complete
        | MemberStatus::Creator => Membership::Member,
    }
}

pub(crate) fn member_wire(m: &MemberRecord) -> GroupMember {
    GroupMember {
        id: m.id,
        name: m.profile.display_name.clone(),
        friend: m.friend,
        me: false,
        owner: m.role == GroupMemberRole::Owner,
        blocked: m.blocked,
        picture: m.profile.image.as_ref().and_then(|i| picture_of(&i.0)),
        ..GroupMember::default()
    }
}

impl<A: AgentPort> Engine<A> {
    /// The user as a member of `g`; the user's member number is 0.
    pub(crate) fn me_wire(&self, g: &GroupRecord) -> GroupMember {
        let u = &self.state.user;
        GroupMember {
            id: 0,
            name: u.name.clone(),
            me: true,
            owner: g.my_role == GroupMemberRole::Owner,
            picture: u.picture.clone(),
            ..GroupMember::default()
        }
    }

    pub(crate) fn group_wire(&self, g: &GroupRecord) -> ChatGroup {
        let current = self
            .state
            .members_of(g.id)
            .filter(|m| m.status.current())
            .count();
        ChatGroup {
            id: g.id,
            name: g.profile.display_name.clone(),
            membership: membership(g.status),
            members: u32::try_from(current + 1).ok(),
            muted: g.muted,
            link: g.link.clone(),
            picture: g.profile.image.as_ref().and_then(|i| picture_of(&i.0)),
            owner: g.my_role == GroupMemberRole::Owner,
            ..ChatGroup::default()
        }
    }
}
