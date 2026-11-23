//! A member's role, restriction or removal (Subscriber.hs `xGrpMemRole`, `xGrpMemRestrict`, `xGrpMemDel`, `xGrpLeave`; codec: a_groups_story_is_told_in_notices_…).

use zaun_chat_wire::model::{ChatNotice, ChatRef, Membership, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request};

use crate::chat::ChatEvent;
use crate::chat::group::{GroupMemberRole, MemberRestrictionStatus, MemberRestrictions};
use crate::chat::ids::MemberId;
use crate::engine::agent_port::ConnId;
use crate::engine::records::{ItemRecord, MemberStatus};
use crate::engine::tests::{Call, T, messages};

struct Team {
    t: T,
    g: i64,
    /// The owner who invited the user.
    bob: (i64, ConnId),
    carol: (i64, ConnId),
    dave: (i64, ConnId),
}

/// The user is a plain member; bob is the owner, carol and dave members.
fn team() -> Team {
    let mut t = T::started();
    let (g, m) = t.group("team", &["bob", "carol", "dave"], GroupMemberRole::Member);
    Team {
        t,
        g,
        bob: m[0],
        carol: m[1],
        dave: m[2],
    }
}

fn id_of(t: &T, g: i64, member: i64) -> MemberId {
    t.e.member(g, member).unwrap().member_id
}

fn set_role(t: &mut T, g: i64, member: i64, role: GroupMemberRole) {
    let mut m = t.e.member(g, member).unwrap();
    m.role = role;
    t.e.save_member(m).unwrap();
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

fn del(id: MemberId, messages: bool) -> ChatEvent {
    ChatEvent::GrpMemDel {
        member_id: id,
        messages,
        roster_version: None,
    }
}

fn role(id: MemberId, role: GroupMemberRole) -> ChatEvent {
    ChatEvent::GrpMemRole {
        member_id: id,
        role,
        member_key: None,
        roster_version: None,
    }
}

#[test]
fn a_member_removed_by_an_admin_is_told_and_loses_the_connection() {
    let mut x = team();
    let id = id_of(&x.t, x.g, x.carol.0);
    let out = x.t.peer_says(&x.bob.1, 1, del(id, false));
    assert_eq!(
        notices(&out),
        [ChatNotice::MemberRemoved {
            by: Some("bob".into()),
            who: "carol".into()
        }]
    );
    let carol = x.t.e.member(x.g, x.carol.0).unwrap();
    assert_eq!((carol.status, carol.conn), (MemberStatus::Removed, None));
    assert!(
        x.t.e
            .agent
            .calls
            .contains(&Call::Delete { conn: x.carol.1 })
    );
    let out = x.t.req(Request::Members(x.g));
    let Some(Event::Members { members, .. }) = out.first() else {
        panic!("{out:?}")
    };
    assert!(members.iter().all(|m| m.name != "carol"));
}

#[test]
fn the_same_removal_arriving_twice_is_told_once() {
    let mut x = team();
    let id = id_of(&x.t, x.g, x.carol.0);
    x.t.peer_says(&x.bob.1, 1, del(id.clone(), false));
    let out = x.t.peer_says(&x.bob.1, 2, del(id, false));
    assert!(notices(&out).is_empty());
}

#[test]
fn a_removal_by_a_plain_member_or_of_a_higher_role_is_dropped() {
    let mut x = team();
    let dave = id_of(&x.t, x.g, x.dave.0);
    x.t.peer_says(&x.carol.1, 1, del(dave, false));
    assert_eq!(
        x.t.e.member(x.g, x.dave.0).unwrap().status,
        MemberStatus::Connected
    );
    // An admin may not remove the owner.
    set_role(&mut x.t, x.g, x.carol.0, GroupMemberRole::Admin);
    let bob = id_of(&x.t, x.g, x.bob.0);
    x.t.peer_says(&x.carol.1, 2, del(bob, false));
    assert_eq!(
        x.t.e.member(x.g, x.bob.0).unwrap().status,
        MemberStatus::Connected
    );
}

#[test]
fn a_removal_with_messages_marks_the_members_messages_deleted() {
    let mut x = team();
    let chat = ChatRef::Group(x.g);
    let mut item = ItemRecord::new(
        MessageAuthor::Member("carol".into()),
        MessageBody::Text("hello".into()),
        1,
    );
    item.member = Some(x.carol.0);
    let mut other = item.clone();
    other.member = Some(x.dave.0);
    other.author = MessageAuthor::Member("dave".into());
    let mine = x.t.e.add_item(chat, &item).unwrap().id;
    let theirs = x.t.e.add_item(chat, &other).unwrap().id;
    let id = id_of(&x.t, x.g, x.carol.0);
    let out = x.t.peer_says(&x.bob.1, 1, del(id, true));
    let updated: Vec<_> = out
        .iter()
        .filter_map(|e| {
            if let Event::Updated(m) = e {
                Some(m.clone())
            } else {
                None
            }
        })
        .flatten()
        .collect();
    assert_eq!(updated.len(), 1);
    assert_eq!(
        (updated[0].id, &updated[0].body),
        (mine, &MessageBody::Deleted)
    );
    let kept = x.t.e.items_page(chat, None, 50).unwrap();
    assert!(
        kept.iter()
            .any(|m| m.id == theirs && m.body == MessageBody::Text("hello".into()))
    );
}

#[test]
fn the_user_removed_by_an_admin_loses_every_connection() {
    let mut x = team();
    let me = x.t.e.group(x.g).unwrap().my_member_id;
    let out = x.t.peer_says(&x.bob.1, 1, del(me, false));
    assert_eq!(notices(&out), [ChatNotice::YouRemoved("bob".into())]);
    assert_eq!(x.t.e.group(x.g).unwrap().status, MemberStatus::Removed);
    for (_, conn) in [x.bob, x.carol, x.dave] {
        assert!(x.t.e.agent.calls.contains(&Call::Delete { conn }));
    }
    let out = x.t.req(Request::Refresh);
    let Some(Event::Chats { groups, .. }) = out.first() else {
        panic!("{out:?}")
    };
    assert_eq!(groups[0].membership, Membership::Left);
}

#[test]
fn a_member_who_leaves_is_marked_left_and_their_connection_is_deleted() {
    let mut x = team();
    let out = x.t.peer_says(&x.carol.1, 1, ChatEvent::GrpLeave);
    assert_eq!(notices(&out), [ChatNotice::MemberLeft("carol".into())]);
    let carol = x.t.e.member(x.g, x.carol.0).unwrap();
    assert_eq!((carol.status, carol.conn), (MemberStatus::Left, None));
    assert!(
        x.t.e
            .agent
            .calls
            .contains(&Call::Delete { conn: x.carol.1 })
    );
    let out = x.t.peer_says(&x.bob.1, 2, ChatEvent::GrpLeave);
    assert_eq!(notices(&out), [ChatNotice::MemberLeft("bob".into())]);
}

#[test]
fn an_owner_changes_a_members_role_and_a_member_may_not() {
    let mut x = team();
    let carol = id_of(&x.t, x.g, x.carol.0);
    x.t.peer_says(&x.dave.1, 1, role(carol.clone(), GroupMemberRole::Admin));
    assert_eq!(
        x.t.e.member(x.g, x.carol.0).unwrap().role,
        GroupMemberRole::Member
    );
    x.t.peer_says(&x.bob.1, 2, role(carol, GroupMemberRole::Admin));
    assert_eq!(
        x.t.e.member(x.g, x.carol.0).unwrap().role,
        GroupMemberRole::Admin
    );
}

#[test]
fn a_moderator_changes_roles_only_up_to_member() {
    let mut x = team();
    set_role(&mut x.t, x.g, x.carol.0, GroupMemberRole::Moderator);
    let dave = id_of(&x.t, x.g, x.dave.0);
    x.t.peer_says(&x.carol.1, 1, role(dave.clone(), GroupMemberRole::Observer));
    assert_eq!(
        x.t.e.member(x.g, x.dave.0).unwrap().role,
        GroupMemberRole::Observer
    );
    x.t.peer_says(&x.carol.1, 2, role(dave, GroupMemberRole::Admin));
    assert_eq!(
        x.t.e.member(x.g, x.dave.0).unwrap().role,
        GroupMemberRole::Observer
    );
}

#[test]
fn the_users_own_role_changes_when_an_admin_says_so_and_a_relay_role_is_never_given() {
    let mut x = team();
    let me = x.t.e.group(x.g).unwrap().my_member_id;
    x.t.peer_says(&x.bob.1, 1, role(me.clone(), GroupMemberRole::Admin));
    assert_eq!(x.t.e.group(x.g).unwrap().my_role, GroupMemberRole::Admin);
    x.t.peer_says(&x.bob.1, 2, role(me, GroupMemberRole::Relay));
    assert_eq!(x.t.e.group(x.g).unwrap().my_role, GroupMemberRole::Admin);
}

#[test]
fn a_restriction_is_checked_and_never_touches_the_users_local_block() {
    let mut x = team();
    let carol = id_of(&x.t, x.g, x.carol.0);
    let restrict = ChatEvent::GrpMemRestrict {
        member_id: carol,
        restrictions: MemberRestrictions {
            restriction: MemberRestrictionStatus::Blocked,
        },
    };
    let out = x.t.peer_says(&x.bob.1, 1, restrict);
    assert!(notices(&out).is_empty());
    assert!(!x.t.e.member(x.g, x.carol.0).unwrap().blocked);
}

#[test]
fn roles_are_checked_on_the_author_of_a_forwarded_event_not_on_the_forwarder() {
    use crate::chat::ChatMessage;
    use crate::engine::groups::GroupCtx;
    let mut x = team();
    let dave = id_of(&x.t, x.g, x.dave.0);
    let ctx = |sender: i64, author: i64| GroupCtx {
        group: x.g,
        sender,
        author,
        forwarded: true,
    };
    // Carol, a plain member, forwards what the owner bob wrote.
    let msg = ChatMessage::new(del(dave.clone(), false)).unwrap();
    x.t.e.on_group_event(ctx(x.carol.0, x.bob.0), msg).unwrap();
    assert_eq!(
        x.t.e.member(x.g, x.dave.0).unwrap().status,
        MemberStatus::Removed
    );
    // The owner bob forwards what the plain member carol wrote.
    let carol = id_of(&x.t, x.g, x.carol.0);
    let msg = ChatMessage::new(del(carol, false)).unwrap();
    assert!(x.t.e.on_group_event(ctx(x.bob.0, x.carol.0), msg).is_err());
    assert_eq!(
        x.t.e.member(x.g, x.carol.0).unwrap().status,
        MemberStatus::Connected
    );
}
