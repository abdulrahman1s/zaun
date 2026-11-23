//! Member roles in order, and what a role may do (Types/Shared.hs `GroupMemberRole`, Commands.hs `assertUserGroupRole`, Internal.hs `roleRequiredToChange`).

use crate::chat::group::GroupMemberRole;
use crate::engine::error::{Code, Result, fail};
use crate::engine::records::{GroupRecord, MemberStatus};

/// The derived `Ord` of the Haskell type: an unknown role is the lowest.
#[must_use]
pub(crate) const fn rank(role: &GroupMemberRole) -> u8 {
    match role {
        GroupMemberRole::Unknown(_) => 0,
        GroupMemberRole::Relay => 1,
        GroupMemberRole::Observer => 2,
        GroupMemberRole::Author => 3,
        GroupMemberRole::Member => 4,
        GroupMemberRole::Moderator => 5,
        GroupMemberRole::Admin => 6,
        GroupMemberRole::Owner => 7,
    }
}

#[must_use]
pub(crate) const fn at_least(role: &GroupMemberRole, min: &GroupMemberRole) -> bool {
    rank(role) >= rank(min)
}

/// `roleRequiredToChange`: moderators change roles only up to member.
pub(crate) fn role_required_to_change(
    from: &GroupMemberRole,
    to: &GroupMemberRole,
) -> GroupMemberRole {
    let member = GroupMemberRole::Member;
    if at_least(&member, from) && at_least(&member, to) {
        return GroupMemberRole::Moderator;
    }
    [from, to, &GroupMemberRole::Admin]
        .into_iter()
        .max_by_key(|r| rank(r))
        .cloned()
        .unwrap_or(GroupMemberRole::Admin)
}

/// `memberActive` of the user's own membership.
const fn active(status: MemberStatus) -> bool {
    matches!(
        status,
        MemberStatus::PendingApproval
            | MemberStatus::PendingReview
            | MemberStatus::Connected
            | MemberStatus::Complete
            | MemberStatus::Creator
    )
}

/// `assertUserGroupRole`: what the user needs to change the group.
pub(crate) fn assert_user_role(g: &GroupRecord, min: &GroupMemberRole) -> Result<()> {
    if !at_least(&g.my_role, min) {
        return Err(fail(Code::RequestFailed, "insufficient role in the group"));
    }
    if !active(g.status) {
        return Err(fail(
            Code::RequestFailed,
            "not an active member of the group",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moderator_changes_roles_only_up_to_member() {
        use GroupMemberRole::{Admin, Member, Moderator, Observer, Owner};
        assert_eq!(role_required_to_change(&Observer, &Member), Moderator);
        assert_eq!(role_required_to_change(&Member, &Moderator), Admin);
        assert_eq!(role_required_to_change(&Admin, &Member), Admin);
        assert_eq!(role_required_to_change(&Member, &Owner), Owner);
        assert_eq!(role_required_to_change(&Owner, &Member), Owner);
    }

    #[test]
    fn roles_rank_as_the_reference_orders_them() {
        use GroupMemberRole::{Admin, Member, Moderator, Observer, Owner, Relay, Unknown};
        let ranked = [
            Unknown("x".into()),
            Relay,
            Observer,
            Member,
            Moderator,
            Admin,
            Owner,
        ];
        assert!(ranked.windows(2).all(|w| rank(&w[0]) < rank(&w[1])));
    }
}
