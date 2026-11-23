//! The user makes a group and invites a friend: NewGroup, AddMember, the invitee's CONF and CON (Commands.hs `APINewGroup`, `APIAddMember`; Subscriber.hs:797-808).

use zaun_chat_wire::model::{ChatNotice, ChatRef, Membership};
use zaun_chat_wire::{Event, Request};

use super::members_support::{allowed, conn_info, invitations_made, link, notices, only, sent_on};
use crate::chat::ChatEvent;
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::chat::version::VersionRange;
use crate::engine::agent_port::{AgentEvent, ConnId};
use crate::engine::records::{MemberCategory, MemberStatus};
use crate::engine::tests::T;

/// alice's new group "team" with bob invited and his link made: (group, bob's member, member conn, bob's friend conn).
pub fn invited_bob(t: &mut T) -> (i64, i64, ConnId, ConnId) {
    let (bob, bob_conn) = t.friend("bob");
    t.req(Request::NewGroup("team".into()));
    let g = *t.e.state.groups.keys().last().unwrap();
    t.req(Request::AddMember {
        group: g,
        friend: bob,
    });
    let m =
        t.e.state
            .members_of(g)
            .find(|m| m.friend == Some(bob))
            .unwrap()
            .clone();
    let conn = m.conn.unwrap();
    t.agent(AgentEvent::Invitation {
        conn,
        full: link(1),
        short: None,
    });
    (g, m.id, conn, bob_conn)
}

pub fn acpt(t: &T, g: i64, m: i64, name: &str) -> Vec<u8> {
    conn_info(ChatEvent::GrpAcpt {
        member_id: t.e.state.members[&(g, m)].member_id.clone(),
        member_key: Some("bob-key".into()),
        profile: Some(Profile::new(name, "")),
    })
}

#[test]
fn a_new_group_has_the_user_as_its_creator_and_owner_and_says_it_was_made() {
    let mut t = T::started();
    let out = t.req(Request::NewGroup("  team ".into()));
    assert_eq!(notices(&out), [ChatNotice::GroupCreated]);
    let g = t.e.state.groups.values().next().unwrap().clone();
    assert_eq!(g.profile.display_name, "team");
    assert_eq!(
        (g.my_role, g.status),
        (GroupMemberRole::Owner, MemberStatus::Creator)
    );
    // The old glue's profile: no preferences, so history and direct messages are off.
    assert_eq!(g.profile.group_preferences, None);
    let groups = out.iter().find_map(|e| match e {
        Event::Chats { groups, .. } => Some(groups.clone()),
        _ => None,
    });
    let team = only(groups.unwrap());
    assert_eq!(
        (team.owner, team.membership, team.members),
        (true, Membership::Member, Some(1))
    );
}

#[test]
fn a_group_needs_a_name() {
    let mut t = T::started();
    let out = t.req(Request::NewGroup("  ".into()));
    assert!(out.iter().any(|e| matches!(e, Event::Failed { .. })));
    assert!(t.e.state.groups.is_empty());
}

#[test]
fn adding_a_friend_makes_an_invitee_and_sends_x_grp_inv_once_its_link_is_made() {
    let mut t = T::started();
    let (bob, bob_conn) = t.friend("bob");
    t.req(Request::NewGroup("team".into()));
    let g = *t.e.state.groups.keys().next().unwrap();
    t.req(Request::AddMember {
        group: g,
        friend: bob,
    });
    let m = t.e.state.members_of(g).next().unwrap().clone();
    assert_eq!(
        (m.category, m.status, m.friend),
        (MemberCategory::Invitee, MemberStatus::Invited, Some(bob))
    );
    assert_eq!(m.role, GroupMemberRole::Member);
    assert!(
        sent_on(&t, bob_conn).is_empty(),
        "nothing goes before the link"
    );
    let out = t.agent(AgentEvent::Invitation {
        conn: m.conn.unwrap(),
        full: link(1),
        short: None,
    });
    let ChatEvent::GrpInv(inv) = only(sent_on(&t, bob_conn)) else {
        panic!()
    };
    let gr = &t.e.state.groups[&g];
    assert_eq!(inv.from_member.member_id, gr.my_member_id);
    assert_eq!(inv.from_member.member_role, GroupMemberRole::Owner);
    assert_eq!(inv.invited_member.member_id, m.member_id);
    assert_eq!(inv.conn_request, link(1));
    assert_eq!(inv.group_profile.display_name, "team");
    assert_eq!(
        inv.from_member_key, None,
        "member keys are optional in classic groups"
    );
    let shown = crate::engine::tests::messages(&out);
    assert_eq!(shown[0].chat, ChatRef::Direct(bob));
    assert_eq!(
        notices(&out),
        [ChatNotice::GroupInvitation {
            group: "team".into(),
            sent: true
        }]
    );
}

#[test]
fn only_an_admin_adds_members_and_only_a_friend_who_is_connected() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    let (g, _) = t.group("club", &["alice"], GroupMemberRole::Member);
    let out = t.req(Request::AddMember {
        group: g,
        friend: bob,
    });
    assert!(out.iter().any(|e| matches!(e, Event::Failed { .. })));
    t.req(Request::NewGroup("team".into()));
    let mine = *t.e.state.groups.keys().last().unwrap();
    t.e.state.friends.get_mut(&bob).unwrap().ready = false;
    let out = t.req(Request::AddMember {
        group: mine,
        friend: bob,
    });
    assert!(out.iter().any(|e| matches!(e, Event::Failed { .. })));
    assert_eq!(invitations_made(&t), 1, "only the friend's own invitation");
}

#[test]
fn adding_an_invited_friend_again_resends_the_same_link_and_a_member_is_not_added_twice() {
    let mut t = T::started();
    let (g, _, _, bob_conn) = invited_bob(&mut t);
    let bob = t.e.state.friend_of(&bob_conn).unwrap();
    t.req(Request::AddMember {
        group: g,
        friend: bob,
    });
    let invs: Vec<String> = sent_on(&t, bob_conn)
        .into_iter()
        .filter_map(|e| match e {
            ChatEvent::GrpInv(i) => Some(i.conn_request),
            _ => None,
        })
        .collect();
    assert_eq!(invs, [link(1), link(1)]);
    assert_eq!(t.e.state.members_of(g).count(), 1);
    assert_eq!(invitations_made(&t), 2, "the friend's and the member's");
}

#[test]
fn the_hosts_conf_checks_the_member_id_and_answers_with_x_grp_mem_info_not_x_ok() {
    let mut t = T::started();
    let (g, m, conn, _) = invited_bob(&mut t);
    let wrong = conn_info(ChatEvent::GrpAcpt {
        member_id: MemberId::mint().unwrap(),
        member_key: None,
        profile: None,
    });
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: wrong,
    });
    assert!(allowed(&t).is_empty());
    assert_eq!(t.e.state.members[&(g, m)].status, MemberStatus::Invited);
    let right = acpt(&t, g, m, "bobby");
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: right.clone(),
    });
    let (c, ChatEvent::GrpMemInfo { member_id, profile }) = only(allowed(&t)) else {
        panic!()
    };
    assert_eq!(c, conn);
    assert_eq!(member_id, t.e.state.groups[&g].my_member_id);
    assert_eq!(profile.display_name, "alice");
    assert_eq!(
        profile.preferences, None,
        "the handshake profile is redacted"
    );
    let rec = &t.e.state.members[&(g, m)];
    assert_eq!(rec.status, MemberStatus::Accepted);
    assert_eq!(rec.key.as_deref(), Some("bob-key"));
    assert_eq!(rec.profile.display_name, "bobby");
    assert_eq!(rec.chat_v, Some(VersionRange::new(9, 22)));
    // A repeated CONF is not answered twice.
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: right,
    });
    assert_eq!(allowed(&t).len(), 1);
}

#[test]
fn a_restart_between_the_invitation_and_its_acceptance_keeps_the_invitee() {
    let mut t = T::started();
    let (g, m, conn, _) = invited_bob(&mut t);
    let mut t = t.restarted();
    let info = acpt(&t, g, m, "bob");
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: info,
    });
    assert_eq!(allowed(&t).len(), 1);
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(notices(&out), [ChatNotice::MemberJoined("bob".into())]);
    assert_eq!(t.e.state.members[&(g, m)].status, MemberStatus::Connected);
}

#[test]
fn a_signed_x_grp_acpt_is_read_like_a_plain_one() {
    use crate::chat::element::encode_signed_element;
    use crate::chat::signed::{ChatBinding, MsgSignature, SignedMsg};
    let mut t = T::started();
    let (g, m, conn, _) = invited_bob(&mut t);
    // 7.1.4 signs it only when the profile presents a badge; the key arrives in it, so
    // nothing is checked here (Subscriber.hs:800-806).
    let signed = SignedMsg {
        binding: ChatBinding::Group,
        signatures: vec![MsgSignature {
            signature: vec![7; 64],
        }],
        signed_body: acpt(&t, g, m, "bob"),
    };
    let info = encode_signed_element(&signed).unwrap();
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: info,
    });
    assert_eq!(allowed(&t).len(), 1);
    assert_eq!(t.e.state.members[&(g, m)].status, MemberStatus::Accepted);
}

#[test]
fn a_friend_already_in_the_group_is_not_added_again() {
    let mut t = T::started();
    let (g, m, conn, bob_conn) = invited_bob(&mut t);
    let bob = t.e.state.friend_of(&bob_conn).unwrap();
    let info = acpt(&t, g, m, "bob");
    t.agent(AgentEvent::Conf {
        conn,
        conn_info: info,
    });
    t.agent(AgentEvent::Con { conn });
    let out = t.req(Request::AddMember {
        group: g,
        friend: bob,
    });
    assert!(out.iter().any(|e| matches!(e, Event::Failed { .. })));
    assert_eq!(t.e.state.members_of(g).count(), 1);
}
