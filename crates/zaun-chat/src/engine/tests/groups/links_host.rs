//! Someone asks through the group's link (Subscriber.hs `processContactConnMessage` REQ, Internal.hs `acceptGroupJoinRequestAsync`).

use super::links::made;
use crate::chat::group::{GroupAcceptance, GroupMemberRole};
use crate::chat::ids::XContactId;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage, parse_chat_messages};
use crate::engine::agent_port::{AgentEvent, ConnId};
use crate::engine::records::{MemberCategory, MemberRecord, MemberStatus};
use crate::engine::tests::{Call, T};

fn contact(name: &str) -> Vec<u8> {
    let msg = ChatMessage::without_id(ChatEvent::Contact {
        profile: Profile::new(name, ""),
        member_key: None,
        contact_req_id: Some(XContactId(vec![1; 16])),
        welcome_msg_id: None,
        request_msg: None,
    });
    msg.to_json().unwrap()
}

fn asks(t: &mut T, link: ConnId, conn_info: Vec<u8>) {
    t.agent(AgentEvent::Req {
        conn: link,
        invitation: vec![7],
        conn_info,
        pq: false,
    });
}

fn member_named(t: &T, group: i64, name: &str) -> MemberRecord {
    t.e.state
        .members_of(group)
        .find(|m| m.profile.display_name == name)
        .cloned()
        .unwrap()
}

#[test]
fn someone_asking_through_the_link_is_accepted_as_a_member_with_x_grp_link_inv_and_pq_off() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    let link = made(&mut t, g);
    asks(&mut t, link, contact("dave"));

    let Some(Call::AcceptRequest {
        invitation,
        conn_info,
    }) = t.e.agent.calls.last().cloned()
    else {
        panic!("{:?}", t.e.agent.calls)
    };
    assert_eq!(invitation, [7]);
    assert_eq!(t.e.agent.pq_seen.last(), Some(&("accept", false)));
    let [x] = parse_chat_messages(&conn_info).unwrap().try_into().unwrap();
    let ChatEvent::GrpLinkInv(inv) = x.event else {
        panic!("not an x.grp.link.inv")
    };
    let group = t.e.group(g).unwrap();
    assert_eq!(inv.from_member.member_id, group.my_member_id);
    assert_eq!(inv.from_member.member_role, GroupMemberRole::Owner);
    assert_eq!(inv.from_member_name, "alice");
    assert_eq!(inv.invited_member.member_role, GroupMemberRole::Member);
    assert_eq!(inv.accepted, Some(GroupAcceptance::Accepted));
    assert_eq!(inv.group_profile.display_name, "team");
    assert_eq!(inv.group_size, Some(2), "the user and bob");
    assert!(x.msg_id.is_none(), "conn-info carries no message id");

    let dave = member_named(&t, g, "dave");
    assert_eq!(dave.member_id, inv.invited_member.member_id);
    assert_eq!(
        (dave.category, dave.status, dave.role, dave.via_link),
        (
            MemberCategory::Invitee,
            MemberStatus::Accepted,
            GroupMemberRole::Member,
            true
        )
    );
    let conn = dave.conn.unwrap();
    assert_eq!(t.e.state.member_of(&conn), Some((g, dave.id)));
    assert!(dave.chat_v.is_some());
}

#[test]
fn a_request_after_the_user_stopped_being_admin_is_dropped() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    let link = made(&mut t, g);
    let mut group = t.e.group(g).unwrap();
    group.my_role = GroupMemberRole::Member;
    t.e.save_group(group).unwrap();
    asks(&mut t, link, contact("dave"));
    assert!(t.e.agent.calls.contains(&Call::RejectRequest {
        invitation: vec![7]
    }));
    assert!(
        !t.e.agent
            .calls
            .iter()
            .any(|c| matches!(c, Call::AcceptRequest { .. }))
    );
    assert_eq!(t.e.state.members_of(g).count(), 1);
}

#[test]
fn a_request_that_is_not_a_join_is_dropped() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    let link = made(&mut t, g);
    let junk = ChatMessage::without_id(ChatEvent::Ok).to_json().unwrap();
    asks(&mut t, link, junk);
    assert!(t.e.agent.calls.contains(&Call::RejectRequest {
        invitation: vec![7]
    }));
    assert_eq!(t.e.state.members_of(g).count(), 1);
}

#[test]
fn the_host_sends_its_profile_to_a_member_who_came_by_link_then_has_it_introduced() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    let link = made(&mut t, g);
    asks(&mut t, link, contact("dave"));
    let dave = member_named(&t, g, "dave");
    let introduce = t.e.on_link_member_con(g, dave.id).unwrap();
    assert!(introduce);
    let (conn, body) = t.e.agent.sent().pop().unwrap();
    assert_eq!(Some(conn), dave.conn);
    let [x] = parse_chat_messages(&body).unwrap().try_into().unwrap();
    let ChatEvent::GrpLinkMem { profile, .. } = x.event else {
        panic!("not an x.grp.link.mem")
    };
    assert_eq!(profile.display_name, "alice");
}

#[test]
fn a_member_who_did_not_come_by_link_gets_no_x_grp_link_mem() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["bob"], GroupMemberRole::Owner);
    assert!(t.e.on_link_member_con(g, members[0].0).unwrap());
    assert!(t.e.agent.sent().is_empty());
}
