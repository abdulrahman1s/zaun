//! The members of a group (codec: members_list_the_user_first_with_owner_and_friend).

use zaun_chat_wire::{Event, Request};

use crate::chat::group::GroupMemberRole;
use crate::engine::records::MemberStatus;
use crate::engine::tests::T;

fn listed(out: &[Event]) -> Vec<zaun_chat_wire::model::GroupMember> {
    out.iter()
        .find_map(|e| match e {
            Event::Members { members, .. } => Some(members.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn members_list_the_user_first_with_owner_and_friend() {
    let mut t = T::started();
    let (bob_friend, _) = t.friend("bob");
    let (g, members) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let mut bob = t.e.member(g, members[0].0).unwrap();
    bob.friend = Some(bob_friend);
    t.e.save_member(bob).unwrap();
    let list = listed(&t.req(Request::Members(g)));
    assert_eq!(list.len(), 3);
    assert!(list[0].me && list[0].owner && list[0].name == "alice" && list[0].id == 0);
    assert_eq!(
        (list[1].name.as_str(), list[1].friend),
        ("bob", Some(bob_friend))
    );
    assert!(!list[1].owner && !list[1].blocked && !list[1].me);
}

#[test]
fn the_member_who_made_the_group_shows_as_owner() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["bob"], GroupMemberRole::Member);
    let list = listed(&t.req(Request::Members(g)));
    assert!(!list[0].owner);
    assert!(list[1].owner, "bob invited the user, as owner");
    assert_eq!(list[1].id, members[0].0);
}

#[test]
fn a_member_who_left_or_was_removed_is_not_listed_and_a_blocked_one_is_marked() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["bob", "carol", "dave"], GroupMemberRole::Owner);
    for (i, status) in [(0, MemberStatus::Left), (1, MemberStatus::Removed)] {
        let mut m = t.e.member(g, members[i].0).unwrap();
        m.status = status;
        t.e.save_member(m).unwrap();
    }
    let mut dave = t.e.member(g, members[2].0).unwrap();
    dave.blocked = true;
    t.e.save_member(dave).unwrap();
    let list = listed(&t.req(Request::Members(g)));
    let names: Vec<_> = list.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["alice", "dave"]);
    assert!(list[1].blocked);
}

#[test]
fn a_group_not_in_the_list_has_no_members_to_show() {
    let mut t = T::started();
    let out = t.req(Request::Members(99));
    assert!(matches!(out.as_slice(), [Event::Failed { .. }]));
}
