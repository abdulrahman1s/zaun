//! Who may do what in a group: the group's feature preferences, with the role order from `rank.rs` (Types/Shared.hs `GroupMemberRole`; Types/Preferences.hs `groupFeatureMemberAllowed'`).

use crate::chat::group::GroupMemberRole;
use crate::chat::group_prefs::{FeatureEnabled, GroupFeature, GroupPreferences};
use crate::engine::records::GroupRecord;

pub(crate) use super::rank::{at_least, rank};

/// The group features this engine acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Feature {
    FullDelete,
    Reactions,
    Files,
    Voice,
}

fn pick(p: &GroupPreferences, f: Feature) -> Option<&GroupFeature> {
    match f {
        Feature::FullDelete => p.full_delete.as_ref(),
        Feature::Reactions => p.reactions.as_ref(),
        Feature::Files => p.files.as_ref(),
        Feature::Voice => p.voice.as_ref(),
    }
}

/// `groupFeatureMemberAllowed'` over `fullGroupPreferences`: the group's own setting, else the
/// default, on, and `role` at least the one it names.
#[must_use]
pub(crate) fn allowed(g: &GroupRecord, f: Feature, role: &GroupMemberRole) -> bool {
    let defaults = GroupPreferences::default_full();
    let own = g
        .profile
        .group_preferences
        .as_ref()
        .and_then(|p| pick(p, f));
    let Some(pref) = own.or_else(|| pick(&defaults, f)) else {
        return false;
    };
    let min = pref.role.as_deref().map(GroupMemberRole::from_wire);
    pref.enable == FeatureEnabled::On && min.is_none_or(|m| at_least(role, &m))
}

/// `groupFeatureAllowed`: on for the group, whoever uses it.
#[must_use]
pub(crate) fn on(g: &GroupRecord, f: Feature) -> bool {
    allowed(g, f, &GroupMemberRole::Owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::group_prefs::GroupFeature;

    fn group(files: GroupFeature) -> GroupRecord {
        let mut p = GroupPreferences::default_full();
        p.files = Some(files);
        let mut g: GroupRecord = serde_json::from_value(serde_json::json!({
            "id": 1, "profile": {"displayName": "g", "fullName": ""},
            "my_member_id": "AA==", "my_role": "member", "status": "connected",
            "host": null, "invitation": null, "invited_by_friend": null, "since": 0,
            "muted": false, "read_up_to": 0, "unread": 0, "link": null, "link_conn": null
        }))
        .unwrap();
        g.profile.group_preferences = Some(p);
        g
    }

    #[test]
    fn a_feature_with_a_role_is_allowed_from_that_role_up() {
        let mut f = GroupFeature::new(FeatureEnabled::On);
        f.role = Some("moderator".into());
        let g = group(f);
        assert!(!allowed(&g, Feature::Files, &GroupMemberRole::Member));
        assert!(allowed(&g, Feature::Files, &GroupMemberRole::Moderator));
        assert!(allowed(&g, Feature::Files, &GroupMemberRole::Owner));
        assert!(on(&g, Feature::Files));
        assert!(!on(
            &group(GroupFeature::new(FeatureEnabled::Off)),
            Feature::Files
        ));
    }

    #[test]
    fn roles_rank_from_relay_to_owner_and_an_unknown_one_lowest() {
        let order = [
            "relay",
            "observer",
            "author",
            "member",
            "moderator",
            "admin",
            "owner",
        ];
        let ranks: Vec<u8> = order
            .iter()
            .map(|r| rank(&GroupMemberRole::from_wire(r)))
            .collect();
        assert!(ranks.windows(2).all(|w| w[0] < w[1]));
        assert!(rank(&GroupMemberRole::Unknown("x".into())) < ranks[0]);
    }
}
