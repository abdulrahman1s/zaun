//! The group changed by its owner, and a member's own profile (Subscriber.hs `xGrpDel`, `xGrpInfo`, `xGrpPrefs`, `xInfoMember`; codec: a_groups_story_is_told_in_notices_…).

use zaun_chat_wire::model::{ChatNotice, Membership, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::seed::profile;
use crate::chat::ChatEvent;
use crate::chat::group::GroupMemberRole;
use crate::chat::group_prefs::{FeatureEnabled, GroupFeature, GroupPreferences};
use crate::chat::profile::Profile;
use crate::engine::agent_port::ConnId;
use crate::engine::records::MemberStatus;
use crate::engine::tests::{Call, T, messages};

/// The user is a plain member; bob is the owner who invited them, carol a member.
fn team(t: &mut T) -> (i64, [(i64, ConnId); 2]) {
    let (g, m) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    (g, [m[0], m[1]])
}

fn notices(out: &[Event]) -> Vec<ChatNotice> {
    messages(out)
        .into_iter()
        .filter_map(|m| match m.body {
            MessageBody::Notice(n) => Some(n),
            _ => None,
        })
        .collect()
}

fn membership(t: &mut T, g: i64) -> Membership {
    let out = t.req(Request::Refresh);
    out.iter()
        .find_map(|e| match e {
            Event::Chats { groups, .. } => groups.iter().find(|x| x.id == g).map(|x| x.membership),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_group_deleted_by_its_owner_is_marked_deleted_and_its_connections_go() {
    let mut t = T::started();
    let (g, [(_, bob), (_, carol)]) = team(&mut t);
    let out = t.peer_says(&bob, 1, ChatEvent::GrpDel);
    assert_eq!(notices(&out), [ChatNotice::GroupDeleted]);
    assert_eq!(t.e.group(g).unwrap().status, MemberStatus::GroupDeleted);
    for conn in [bob, carol] {
        assert!(t.e.agent.calls.contains(&Call::Delete { conn }));
        assert!(t.e.state.member_of(&conn).is_none());
    }
    assert_eq!(membership(&mut t, g), Membership::Left);
}

#[test]
fn a_group_deleted_by_someone_who_is_not_its_owner_stays() {
    let mut t = T::started();
    let (g, [_, (_, carol)]) = team(&mut t);
    let out = t.peer_says(&carol, 1, ChatEvent::GrpDel);
    assert!(notices(&out).is_empty());
    assert_eq!(t.e.group(g).unwrap().status, MemberStatus::Connected);
}

#[test]
fn an_owners_new_group_name_is_told_and_a_new_picture_alone_is_not() {
    let mut t = T::started();
    let (g, [(_, bob), _]) = team(&mut t);
    let out = t.peer_says(&bob, 1, ChatEvent::GrpInfo(profile("squad")));
    assert_eq!(
        notices(&out),
        [ChatNotice::GroupNameChanged("squad".into())]
    );
    assert_eq!(t.e.group(g).unwrap().profile.display_name, "squad");

    let mut with_image = profile("squad");
    with_image.image = Some(crate::chat::image::ImageData(
        "data:image/png;base64,AAAA".into(),
    ));
    let out = t.peer_says(&bob, 2, ChatEvent::GrpInfo(with_image));
    assert!(notices(&out).is_empty());
    assert!(t.e.group(g).unwrap().profile.image.is_some());
}

#[test]
fn a_new_group_profile_from_a_member_who_is_not_owner_is_dropped() {
    let mut t = T::started();
    let (g, [_, (_, carol)]) = team(&mut t);
    t.peer_says(&carol, 1, ChatEvent::GrpInfo(profile("mine now")));
    assert_eq!(t.e.group(g).unwrap().profile.display_name, "team");
}

#[test]
fn a_channels_profile_is_not_taken_in_a_group() {
    let mut t = T::started();
    let (g, [(_, bob), _]) = team(&mut t);
    let mut p = profile("team");
    p.public_group = Some(serde_json::json!({"groupType": "channel"}));
    p.full_name = "changed".into();
    t.peer_says(&bob, 1, ChatEvent::GrpInfo(p));
    assert_eq!(t.e.group(g).unwrap().profile.full_name, "");
}

#[test]
fn the_owners_group_preferences_are_stored_and_a_members_are_not() {
    let mut t = T::started();
    let (g, [(_, bob), (_, carol)]) = team(&mut t);
    let prefs = GroupPreferences {
        reactions: Some(GroupFeature::new(FeatureEnabled::Off)),
        ..GroupPreferences::default()
    };
    t.peer_says(&carol, 1, ChatEvent::GrpPrefs(prefs.clone()));
    assert_eq!(t.e.group(g).unwrap().profile.group_preferences, None);
    t.peer_says(&bob, 2, ChatEvent::GrpPrefs(prefs.clone()));
    assert_eq!(t.e.group(g).unwrap().profile.group_preferences, Some(prefs));
}

#[test]
fn a_members_new_name_is_kept_and_told_once() {
    let mut t = T::started();
    let (g, [_, (carol_id, carol)]) = team(&mut t);
    let info = |name: &str| ChatEvent::Info {
        profile: Profile::new(name, ""),
        member_key: None,
    };
    let out = t.peer_says(&carol, 1, info("caroline"));
    assert_eq!(
        notices(&out),
        [ChatNotice::NameChanged {
            from: "carol".into(),
            to: "caroline".into()
        }]
    );
    assert_eq!(
        t.e.member(g, carol_id).unwrap().profile.display_name,
        "caroline"
    );
    let out = t.peer_says(&carol, 2, info("caroline"));
    assert!(notices(&out).is_empty());
}
