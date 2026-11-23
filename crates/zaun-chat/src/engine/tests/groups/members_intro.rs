//! The user as host introduces a new member to the group (Internal.hs `introduceToAll`; Subscriber.hs `xGrpMemInv`, `xGrpMemCon`).

use zaun_chat_wire::Request;
use zaun_chat_wire::model::ChatNotice;

use super::members::acpt;
use super::members_support::{link, notices, only, sent_on};
use crate::chat::ChatEvent;
use crate::chat::group::{GroupMemberRole, IntroInvitation};
use crate::chat::ids::MemberId;
use crate::chat::version::VersionRange;
use crate::engine::agent_port::{AgentEvent, ConnId};
use crate::engine::records::{MemberRelation, MemberStatus};
use crate::engine::tests::T;

/// alice owns "team" with bob and carol connected; dave, her friend, is added and connects:
/// (group, [(bob, conn), (carol, conn)], dave's member, dave's member conn).
fn dave_joins(t: &mut T) -> (i64, Vec<(i64, ConnId)>, i64, ConnId) {
    let (g, members) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let (dave, _) = t.friend("dave");
    t.req(Request::AddMember {
        group: g,
        friend: dave,
    });
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.friend == Some(dave))
            .unwrap()
            .clone();
    let conn = m.conn.unwrap();
    t.agent(AgentEvent::Invitation {
        conn,
        full: link(1),
        short: None,
    });
    let info = acpt(t, g, m.id, "dave");
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: info,
    });
    (g, members, m.id, conn)
}

#[test]
fn on_con_the_invitee_is_announced_to_every_member_and_each_is_introduced_to_it() {
    let mut t = T::started();
    let (g, members, dave, conn) = dave_joins(&mut t);
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(notices(&out), [ChatNotice::MemberJoined("dave".into())]);
    let dave_id = t.e.state.members[&(g, dave)].member_id.clone();
    for (_, c) in &members {
        let ChatEvent::GrpMemNew {
            member_info,
            scope: None,
        } = only(sent_on(&t, *c))
        else {
            panic!()
        };
        assert_eq!(member_info.member_id, dave_id);
        assert_eq!(member_info.v, Some(VersionRange::new(9, 22)));
    }
    let intros: Vec<MemberId> = sent_on(&t, conn)
        .into_iter()
        .map(|e| match e {
            ChatEvent::GrpMemIntro { member_info, .. } => {
                assert!(
                    member_info.v.is_some(),
                    "7.1.4 drops an intro without a range"
                );
                member_info.member_id
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(intros.len(), 2);
    // Both intros went in one batch (`introduceMember` always batches).
    assert_eq!(
        t.e.agent.sent().iter().filter(|(c, _)| *c == conn).count(),
        1
    );
    let mut waiting = t.e.not_yet_connected_to(g, dave);
    waiting.sort_unstable();
    assert_eq!(waiting, members.iter().map(|(m, _)| *m).collect::<Vec<_>>());
    assert_eq!(t.e.not_yet_connected_to(g, members[0].0), [dave]);
}

#[test]
fn a_member_not_reachable_now_is_not_introduced_and_stays_new() {
    let mut t = T::started();
    let (g, members, dave, conn) = dave_joins(&mut t);
    let (carol, carol_conn) = members[1];
    t.e.state.members.get_mut(&(g, carol)).unwrap().status = MemberStatus::Announced;
    t.agent(AgentEvent::Con { conn });
    assert!(sent_on(&t, carol_conn).is_empty());
    assert_eq!(sent_on(&t, conn).len(), 1);
    assert!(!t.e.state.members[&(g, dave)].relations.contains_key(&carol));
}

#[test]
fn a_repeated_con_is_neither_told_nor_introduced_twice() {
    let mut t = T::started();
    let (_, members, _, conn) = dave_joins(&mut t);
    t.agent(AgentEvent::Con { conn });
    let out = t.agent(AgentEvent::Con { conn });
    assert!(notices(&out).is_empty());
    assert_eq!(sent_on(&t, members[0].1).len(), 1);
}

#[test]
fn x_grp_mem_inv_from_the_invitee_goes_to_the_member_it_names_as_x_grp_mem_fwd() {
    let mut t = T::started();
    let (g, members, dave, conn) = dave_joins(&mut t);
    t.agent(AgentEvent::Con { conn });
    let (bob, bob_conn) = members[0];
    let bob_id = t.e.state.members[&(g, bob)].member_id.clone();
    let intro = IntroInvitation {
        group_conn_req: link(2),
    };
    t.peer_says(
        &conn,
        1,
        ChatEvent::GrpMemInv {
            member_id: bob_id,
            intro: intro.clone(),
        },
    );
    let fwd = sent_on(&t, bob_conn).pop().unwrap();
    let ChatEvent::GrpMemFwd {
        member_info,
        intro: got,
    } = fwd
    else {
        panic!("{fwd:?}")
    };
    assert_eq!(
        member_info.member_id,
        t.e.state.members[&(g, dave)].member_id
    );
    assert_eq!(member_info.profile.display_name, "dave");
    assert_eq!(got, intro);
}

#[test]
fn x_grp_mem_inv_from_a_member_the_user_did_not_invite_is_dropped() {
    let mut t = T::started();
    let (g, members, dave, conn) = dave_joins(&mut t);
    t.agent(AgentEvent::Con { conn });
    let (_, bob_conn) = members[0];
    let (_, carol_conn) = members[1];
    // bob is seeded as an invitee too, so make him one the user was introduced to.
    let bob = members[0].0;
    t.e.state.members.get_mut(&(g, bob)).unwrap().category =
        crate::engine::records::MemberCategory::Pre;
    let dave_id = t.e.state.members[&(g, dave)].member_id.clone();
    let before = sent_on(&t, carol_conn).len();
    t.peer_says(
        &bob_conn,
        1,
        ChatEvent::GrpMemInv {
            member_id: dave_id,
            intro: IntroInvitation {
                group_conn_req: link(3),
            },
        },
    );
    assert_eq!(sent_on(&t, carol_conn).len(), before);
    assert_eq!(sent_on(&t, conn).len(), 2, "only the two intros");
}

#[test]
fn x_grp_mem_con_marks_the_pair_connected_so_forwarding_between_them_stops() {
    let mut t = T::started();
    let (g, members, dave, conn) = dave_joins(&mut t);
    t.agent(AgentEvent::Con { conn });
    let (bob, _) = members[0];
    let bob_id = t.e.state.members[&(g, bob)].member_id.clone();
    t.peer_says(&conn, 1, ChatEvent::GrpMemCon { member_id: bob_id });
    assert_eq!(t.e.not_yet_connected_to(g, dave), [members[1].0]);
    assert!(t.e.not_yet_connected_to(g, bob).is_empty());
    assert_eq!(
        t.e.state.members[&(g, bob)].relations[&dave],
        MemberRelation::Connected
    );
}

#[test]
fn introductions_come_back_after_a_restart() {
    let mut t = T::started();
    let (g, _, dave, conn) = dave_joins(&mut t);
    t.agent(AgentEvent::Con { conn });
    let t = t.restarted();
    assert_eq!(t.e.not_yet_connected_to(g, dave).len(), 2);
}

#[test]
fn a_new_member_below_v18_gets_its_intros_as_a_json_array() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob", "carol"], GroupMemberRole::Owner);
    let (dave, _) = t.friend("dave");
    t.req(Request::AddMember {
        group: g,
        friend: dave,
    });
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.friend == Some(dave))
            .unwrap()
            .clone();
    let conn = m.conn.unwrap();
    t.agent(AgentEvent::Invitation {
        conn,
        full: link(1),
        short: None,
    });
    let old = crate::chat::ChatMessage {
        v: Some(VersionRange::new(9, 17)),
        msg_id: None,
        event: ChatEvent::GrpAcpt {
            member_id: m.member_id.clone(),
            member_key: None,
            profile: None,
        },
    };
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: old.to_json().unwrap(),
    });
    t.agent(AgentEvent::Con { conn });
    let (_, body) =
        t.e.agent
            .sent()
            .into_iter()
            .rfind(|(c, _)| *c == conn)
            .unwrap();
    assert_eq!(body[0], b'[');
    assert_eq!(sent_on(&t, conn).len(), 2);
}
