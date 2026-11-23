//! The user leaves a group (Commands.hs `APILeaveGroup`).

use zaun_chat_wire::model::{ChatNotice, Membership, MessageBody};
use zaun_chat_wire::{Event, Request};

use crate::chat::group::GroupMemberRole;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::records::MemberStatus;
use crate::engine::tests::{Call, T, messages};

#[test]
fn leaving_tells_every_member_and_deletes_the_connections_and_says_so() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let out = t.req(Request::LeaveGroup(g));

    let sent = t.e.agent.sent();
    assert_eq!(sent.len(), 2);
    for (conn, body) in &sent {
        assert!(members.iter().any(|(_, c)| c == conn));
        let [x] = parse_chat_messages(body).unwrap().try_into().unwrap();
        assert_eq!(x.event, ChatEvent::GrpLeave);
    }
    for (member, conn) in &members {
        assert!(t.e.agent.calls.contains(&Call::Delete { conn: *conn }));
        assert_eq!(t.e.member(g, *member).unwrap().conn, None);
    }
    let notes: Vec<_> = messages(&out).into_iter().map(|m| m.body).collect();
    assert_eq!(notes, [MessageBody::Notice(ChatNotice::YouLeft)]);
    assert_eq!(t.e.group(g).unwrap().status, MemberStatus::Left);
    let Some(Event::Chats { groups, .. }) = out.iter().find(|e| matches!(e, Event::Chats { .. }))
    else {
        panic!("{out:?}")
    };
    assert_eq!(groups[0].membership, Membership::Left);
}

#[test]
fn the_x_grp_leave_of_the_last_message_goes_out_before_the_connection_is_deleted() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Member);
    t.req(Request::LeaveGroup(g));
    let calls = &t.e.agent.calls;
    let sent = calls
        .iter()
        .position(|c| matches!(c, Call::Send { .. }))
        .unwrap();
    let deleted = calls
        .iter()
        .position(|c| matches!(c, Call::Delete { .. }))
        .unwrap();
    assert!(sent < deleted);
}

#[test]
fn leaving_a_group_already_left_does_nothing_more() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Member);
    t.req(Request::LeaveGroup(g));
    let calls = t.e.agent.calls.len();
    let out = t.req(Request::LeaveGroup(g));
    assert!(messages(&out).is_empty());
    assert_eq!(t.e.agent.calls.len(), calls);
}

#[test]
fn a_group_not_in_the_list_cannot_be_left() {
    let mut t = T::started();
    let out = t.req(Request::LeaveGroup(99));
    assert!(matches!(out.as_slice(), [Event::Failed { .. }]));
}
