//! Introductions as the members see them: the joiner's `x.grp.mem.intro` and `x.grp.mem.inv`, an existing member's `x.grp.mem.new` and `x.grp.mem.fwd`, then CONF/INFO, CON and `x.grp.mem.con` (Subscriber.hs `xGrpMemNew`, `xGrpMemIntro`, `xGrpMemFwd`, :986-1010).

use zaun_chat_wire::model::ChatNotice;

use super::members_join::joined_team;
use super::members_support::{
    allowed, conn_info, info, invitations_made, joined, last_conn, link, notices, only, sent_on,
};
use crate::chat::ChatEvent;
use crate::chat::container::MsgScope;
use crate::chat::group::{GroupMemberRole, IntroInvitation, MemberInfo};
use crate::chat::profile::Profile;
use crate::engine::agent_port::{AgentEvent, ConnId};
use crate::engine::records::{MemberCategory, MemberStatus};
use crate::engine::tests::T;

/// What a host would say about a member the user already knows.
fn known(t: &T, g: i64, m: i64) -> MemberInfo {
    let r = &t.e.state.members[&(g, m)];
    MemberInfo {
        member_id: r.member_id.clone(),
        member_role: r.role.clone(),
        v: Some(crate::chat::version::SUPPORTED_CHAT_VRANGE),
        profile: r.profile.clone(),
        member_key: None,
    }
}

fn intro(member_info: MemberInfo) -> ChatEvent {
    ChatEvent::GrpMemIntro {
        member_info,
        restrictions: None,
    }
}

/// The user, just joined, is introduced to carol; carol's link is made: (group, host conn, carol, carol's conn).
fn introduced_to_carol(t: &mut T) -> (i64, ConnId, i64, ConnId) {
    let (g, _, host_conn) = joined_team(t);
    let carol = info("carol", GroupMemberRole::Member);
    t.peer_says(&host_conn, 10, intro(carol.clone()));
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.member_id == carol.member_id)
            .unwrap()
            .clone();
    (g, host_conn, m.id, m.conn.unwrap())
}

#[test]
fn x_grp_mem_intro_makes_a_pre_member_and_one_link_whose_invitation_goes_to_the_host() {
    let mut t = T::started();
    let (g, host_conn, carol, conn) = introduced_to_carol(&mut t);
    let m = t.e.state.members[&(g, carol)].clone();
    assert_eq!(
        (m.category, m.status),
        (MemberCategory::Pre, MemberStatus::Introduced)
    );
    assert_eq!(t.e.state.member_of(&conn), Some((g, carol)));
    t.agent(AgentEvent::Invitation {
        conn,
        full: link(5),
        short: None,
    });
    let ChatEvent::GrpMemInv { member_id, intro } = only(sent_on(&t, host_conn)) else {
        panic!()
    };
    assert_eq!(member_id, m.member_id);
    assert_eq!(intro.group_conn_req, link(5));
    assert_eq!(
        t.e.state.members[&(g, carol)].status,
        MemberStatus::IntroInvited
    );
    // The link arriving again sends nothing more.
    t.agent(AgentEvent::Invitation {
        conn,
        full: link(5),
        short: None,
    });
    assert_eq!(sent_on(&t, host_conn).len(), 1);
}

#[test]
fn x_grp_mem_intro_is_taken_only_from_the_host_while_an_admin_and_with_a_version() {
    let mut t = T::started();
    let (g, host, host_conn) = joined_team(&mut t);
    let before = invitations_made(&t);
    let mut no_v = info("carol", GroupMemberRole::Member);
    no_v.v = None;
    t.peer_says(&host_conn, 10, intro(no_v));
    t.e.state.members.get_mut(&(g, host)).unwrap().role = GroupMemberRole::Moderator;
    t.peer_says(&host_conn, 11, intro(info("dan", GroupMemberRole::Member)));
    t.e.state.members.get_mut(&(g, host)).unwrap().role = GroupMemberRole::Owner;
    let carol = info("carol", GroupMemberRole::Member);
    t.peer_says(&host_conn, 12, intro(carol.clone()));
    let carol_conn =
        t.e.state
            .members_of(g)
            .find(|m| m.member_id == carol.member_id)
            .unwrap()
            .conn
            .unwrap();
    t.agent(AgentEvent::Con { conn: carol_conn });
    // carol, a pre-member and not the host, introduces someone: dropped.
    t.peer_says(&carol_conn, 1, intro(info("eve", GroupMemberRole::Member)));
    assert_eq!(invitations_made(&t), before + 1);
    assert_eq!(t.e.state.members_of(g).count(), 2);
}

#[test]
fn x_grp_mem_intro_about_a_known_member_is_dropped() {
    let mut t = T::started();
    let (g, host_conn, carol, _) = introduced_to_carol(&mut t);
    let again = known(&t, g, carol);
    let before = invitations_made(&t);
    t.peer_says(&host_conn, 20, intro(again));
    let mut me = info("me", GroupMemberRole::Member);
    me.member_id = t.e.state.groups[&g].my_member_id.clone();
    t.peer_says(&host_conn, 21, intro(me));
    assert_eq!(invitations_made(&t), before, "nor one about the user");
    assert_eq!(t.e.state.members_of(g).count(), 2);
}

#[test]
fn the_introduced_members_conf_is_answered_and_its_con_is_told_to_the_host() {
    let mut t = T::started();
    let (g, host_conn, carol, conn) = introduced_to_carol(&mut t);
    t.agent(AgentEvent::Invitation {
        conn,
        full: link(5),
        short: None,
    });
    let id = t.e.state.members[&(g, carol)].member_id.clone();
    let mem_info = conn_info(ChatEvent::GrpMemInfo {
        member_id: id.clone(),
        profile: Profile::new("caroline", ""),
    });
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: mem_info,
    });
    let (c, ChatEvent::GrpMemInfo { member_id, .. }) = only(allowed(&t)) else {
        panic!()
    };
    assert_eq!(
        (c, member_id),
        (conn, t.e.state.groups[&g].my_member_id.clone())
    );
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(notices(&out), [ChatNotice::MemberJoined("caroline".into())]);
    assert_eq!(
        sent_on(&t, host_conn).pop(),
        Some(ChatEvent::GrpMemCon { member_id: id })
    );
    assert_eq!(
        t.e.state.members[&(g, carol)].status,
        MemberStatus::Connected
    );
}

#[test]
fn x_grp_mem_new_from_an_admin_adds_a_post_member_and_tells_who_added_whom() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice", "bob"], GroupMemberRole::Member);
    let (alice, alice_conn) = members[0];
    let dave = info("dave", GroupMemberRole::Member);
    let out = t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemNew {
            member_info: dave.clone(),
            scope: None,
        },
    );
    assert_eq!(
        notices(&out),
        [ChatNotice::MemberAdded {
            by: Some("alice".into()),
            who: "dave".into()
        }]
    );
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.member_id == dave.member_id)
            .unwrap();
    assert_eq!(
        (m.category, m.status, m.invited_by),
        (MemberCategory::Post, MemberStatus::Announced, Some(alice))
    );
}

#[test]
fn x_grp_mem_new_from_a_plain_member_or_for_a_higher_role_or_about_a_known_member_is_dropped() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice", "bob"], GroupMemberRole::Member);
    let (_, alice_conn) = members[0];
    let (bob, bob_conn) = members[1];
    let out = t.peer_says(
        &bob_conn,
        1,
        ChatEvent::GrpMemNew {
            member_info: info("dave", GroupMemberRole::Member),
            scope: None,
        },
    );
    assert!(notices(&out).is_empty(), "bob is a plain member");
    t.e.state.members.get_mut(&(g, members[0].0)).unwrap().role = GroupMemberRole::Admin;
    let out = t.peer_says(
        &alice_conn,
        2,
        ChatEvent::GrpMemNew {
            member_info: info("eve", GroupMemberRole::Owner),
            scope: None,
        },
    );
    assert!(notices(&out).is_empty(), "an admin can't add an owner");
    let known = known(&t, g, bob);
    let out = t.peer_says(
        &alice_conn,
        3,
        ChatEvent::GrpMemNew {
            member_info: known,
            scope: None,
        },
    );
    assert!(notices(&out).is_empty());
    assert_eq!(t.e.state.members_of(g).count(), 2);
}

#[test]
fn x_grp_mem_fwd_from_the_announcer_joins_its_link_with_x_grp_mem_info() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice"], GroupMemberRole::Member);
    let (alice, alice_conn) = members[0];
    let dave = info("dave", GroupMemberRole::Member);
    t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemNew {
            member_info: dave.clone(),
            scope: None,
        },
    );
    let intro = IntroInvitation {
        group_conn_req: link(7),
    };
    t.peer_says(
        &alice_conn,
        2,
        ChatEvent::GrpMemFwd {
            member_info: dave.clone(),
            intro,
        },
    );
    let (l, ChatEvent::GrpMemInfo { member_id, profile }) = only(joined(&t)) else {
        panic!()
    };
    assert!(l.contains("invitation"));
    assert_eq!(member_id, t.e.state.groups[&g].my_member_id);
    assert_eq!(profile.preferences, None);
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.member_id == dave.member_id)
            .unwrap();
    assert_eq!(
        (m.status, m.invited_by),
        (MemberStatus::IntroInvited, Some(alice))
    );
    assert_eq!(t.e.state.member_of(&m.conn.unwrap()), Some((g, m.id)));
    assert_eq!(m.conn, Some(last_conn(&t)));
}

#[test]
fn x_grp_mem_fwd_from_anyone_but_the_announcer_is_dropped() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice", "bob"], GroupMemberRole::Member);
    let (_, alice_conn) = members[0];
    let (bob, bob_conn) = members[1];
    t.e.state.members.get_mut(&(g, bob)).unwrap().role = GroupMemberRole::Admin;
    let dave = info("dave", GroupMemberRole::Member);
    t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemNew {
            member_info: dave.clone(),
            scope: None,
        },
    );
    let intro = IntroInvitation {
        group_conn_req: link(7),
    };
    t.peer_says(
        &bob_conn,
        1,
        ChatEvent::GrpMemFwd {
            member_info: dave,
            intro,
        },
    );
    assert!(joined(&t).is_empty());
}

#[test]
fn x_grp_mem_fwd_before_x_grp_mem_new_makes_the_member_with_the_sender_as_announcer() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice"], GroupMemberRole::Member);
    let (alice, alice_conn) = members[0];
    let dave = info("dave", GroupMemberRole::Member);
    let intro = IntroInvitation {
        group_conn_req: link(7),
    };
    t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemFwd {
            member_info: dave.clone(),
            intro,
        },
    );
    assert_eq!(joined(&t).len(), 1);
    let out = t.peer_says(
        &alice_conn,
        2,
        ChatEvent::GrpMemNew {
            member_info: dave.clone(),
            scope: None,
        },
    );
    assert!(
        notices(&out).is_empty(),
        "the late announcement finds the member"
    );
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.member_id == dave.member_id)
            .unwrap();
    assert_eq!(
        (m.category, m.invited_by),
        (MemberCategory::Post, Some(alice))
    );
}

#[test]
fn the_new_members_info_and_con_are_told_to_its_announcer_as_x_grp_mem_con() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice"], GroupMemberRole::Member);
    let (_, alice_conn) = members[0];
    let dave = info("dave", GroupMemberRole::Member);
    t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemNew {
            member_info: dave.clone(),
            scope: None,
        },
    );
    let intro = IntroInvitation {
        group_conn_req: link(7),
    };
    t.peer_says(
        &alice_conn,
        2,
        ChatEvent::GrpMemFwd {
            member_info: dave.clone(),
            intro,
        },
    );
    let conn = last_conn(&t);
    let mem_info = conn_info(ChatEvent::GrpMemInfo {
        member_id: dave.member_id.clone(),
        profile: Profile::new("david", ""),
    });
    t.agent(AgentEvent::Info {
        conn,
        conn_info: mem_info,
    });
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(notices(&out), [ChatNotice::MemberJoined("david".into())]);
    assert_eq!(
        sent_on(&t, alice_conn),
        [ChatEvent::GrpMemCon {
            member_id: dave.member_id
        }]
    );
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.conn == Some(conn))
            .unwrap();
    assert_eq!(m.status, MemberStatus::Connected);
}

#[test]
fn x_grp_mem_con_all_is_not_acted_on_as_in_7_1_4() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice", "bob"], GroupMemberRole::Member);
    let (_, alice_conn) = members[0];
    let bob_id = t.e.state.members[&(g, members[1].0)].member_id.clone();
    let before = t.e.state.members.clone();
    t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemConAll { member_id: bob_id },
    );
    assert_eq!(t.e.state.members, before);
    assert!(sent_on(&t, alice_conn).is_empty());
}

#[test]
fn x_grp_mem_fwd_from_a_non_admin_for_a_higher_role_or_about_the_user_is_dropped() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice", "bob"], GroupMemberRole::Member);
    let (alice, alice_conn) = members[0];
    let (_, bob_conn) = members[1];
    let fwd = |member_info| ChatEvent::GrpMemFwd {
        member_info,
        intro: IntroInvitation {
            group_conn_req: link(7),
        },
    };
    t.peer_says(&bob_conn, 1, fwd(info("dave", GroupMemberRole::Member)));
    t.e.state.members.get_mut(&(g, alice)).unwrap().role = GroupMemberRole::Admin;
    t.peer_says(&alice_conn, 1, fwd(info("eve", GroupMemberRole::Owner)));
    let mut me = info("me", GroupMemberRole::Member);
    me.member_id = t.e.state.groups[&g].my_member_id.clone();
    t.peer_says(&alice_conn, 2, fwd(me));
    assert!(joined(&t).is_empty());
    assert_eq!(t.e.state.members_of(g).count(), 2);
}

#[test]
fn a_peers_handshake_must_be_x_grp_mem_info_with_its_own_member_id() {
    let mut t = T::started();
    let (g, _, carol, conn) = introduced_to_carol(&mut t);
    let other = info("mallory", GroupMemberRole::Member);
    let wrong = conn_info(ChatEvent::GrpMemInfo {
        member_id: other.member_id,
        profile: Profile::new("mallory", ""),
    });
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: wrong.clone(),
    });
    let not_info = conn_info(ChatEvent::Info {
        profile: Profile::new("mallory", ""),
        member_key: None,
    });
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: not_info,
    });
    assert!(allowed(&t).is_empty());
    t.agent(AgentEvent::Info {
        conn,
        conn_info: wrong,
    });
    assert_eq!(t.e.state.members[&(g, carol)].profile.display_name, "carol");
}

#[test]
fn x_grp_mem_new_about_the_user_is_ignored_and_one_known_only_as_unknown_is_announced() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice"], GroupMemberRole::Member);
    let (alice, alice_conn) = members[0];
    let mut me = info("me", GroupMemberRole::Member);
    me.member_id = t.e.state.groups[&g].my_member_id.clone();
    let out = t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemNew {
            member_info: me,
            scope: None,
        },
    );
    assert!(notices(&out).is_empty());
    assert_eq!(t.e.state.members_of(g).count(), 1);
    let dave = info("dave", GroupMemberRole::Member);
    let mut seen =
        t.e.new_member(
            g,
            &info("someone", GroupMemberRole::Observer),
            MemberCategory::Post,
            MemberStatus::Unknown,
            None,
        )
        .unwrap();
    seen.member_id = dave.member_id.clone();
    t.e.save_member(seen.clone()).unwrap();
    let out = t.peer_says(
        &alice_conn,
        2,
        ChatEvent::GrpMemNew {
            member_info: dave,
            scope: None,
        },
    );
    assert_eq!(
        notices(&out),
        [ChatNotice::MemberAdded {
            by: Some("alice".into()),
            who: "dave".into()
        }]
    );
    let m = &t.e.state.members[&(g, seen.id)];
    assert_eq!(
        (m.status, m.invited_by, m.role.clone()),
        (
            MemberStatus::Announced,
            Some(alice),
            GroupMemberRole::Member
        )
    );
    assert_eq!(t.e.state.members_of(g).count(), 2);
}

#[test]
fn x_grp_mem_new_into_a_members_support_scope_waits_for_review() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice"], GroupMemberRole::Member);
    let (_, alice_conn) = members[0];
    let dave = info("dave", GroupMemberRole::Member);
    let scope = Some(MsgScope::Member {
        member_id: dave.member_id.clone(),
    });
    t.peer_says(
        &alice_conn,
        1,
        ChatEvent::GrpMemNew {
            member_info: dave.clone(),
            scope,
        },
    );
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.member_id == dave.member_id)
            .unwrap();
    assert_eq!(m.status, MemberStatus::PendingReview);
}

#[test]
fn x_grp_mem_con_about_an_unknown_member_changes_nothing() {
    let mut t = T::started();
    let (g, members) = t.group("team", &["alice", "bob"], GroupMemberRole::Owner);
    let before = t.e.state.members.clone();
    let stranger = info("stranger", GroupMemberRole::Member).member_id;
    t.peer_says(
        &members[0].1,
        1,
        ChatEvent::GrpMemCon {
            member_id: stranger,
        },
    );
    assert_eq!(t.e.state.members, before);
    assert!(t.e.not_yet_connected_to(g, members[0].0).is_empty());
}
