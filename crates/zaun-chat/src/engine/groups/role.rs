//! The role and feature checks of the join path (Subscriber.hs `checkHostRole`; Types/Preferences.hs); the order of roles is `rank.rs`.

use crate::chat::group::GroupMemberRole;
use crate::chat::group_prefs::FeatureEnabled;
use crate::engine::records::GroupRecord;

pub(crate) use super::rank::at_least;

/// `checkHostRole` (Subscriber.hs:3705): who announces or introduces a member must be an admin
/// and hold at least the role it gives that member.
#[must_use]
pub(crate) const fn may_admit(sender: &GroupMemberRole, given: &GroupMemberRole) -> bool {
    at_least(sender, &GroupMemberRole::Admin) && at_least(sender, given)
}

/// `groupFeatureMemberAllowed SGFDirectMessages` (Types/Preferences.hs): on in the group, and the
/// member holds the role it names, if any. Off by default (`defaultGroupPrefs`).
#[must_use]
pub(crate) fn direct_messages_allowed(g: &GroupRecord, role: &GroupMemberRole) -> bool {
    let Some(p) = g
        .profile
        .group_preferences
        .as_ref()
        .and_then(|p| p.direct_messages.as_ref())
    else {
        return false;
    };
    let has_role = p
        .role
        .as_deref()
        .is_none_or(|min| at_least(role, &GroupMemberRole::from_wire(min)));
    p.enable == FeatureEnabled::On && has_role
}

/// `groupFeatureAllowed SGFHistory`: off by default.
#[must_use]
pub(crate) fn history_on(g: &GroupRecord) -> bool {
    g.profile
        .group_preferences
        .as_ref()
        .and_then(|p| p.history.as_ref())
        .is_some_and(|h| h.enable == FeatureEnabled::On)
}
