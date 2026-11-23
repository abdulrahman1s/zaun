//! The user is invited and joins: `x.grp.inv`, JoinGroup with `x.grp.acpt`, the host's INFO and CON (Subscriber.hs `processGroupInvitation`, `xGrpInv`; Commands.hs `APIJoinGroup`).

use zaun_chat_wire::model::{ChatNotice, ChatRef, Membership, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::members_support::{conn_info, joined, link, notices, only};
use super::seed::profile;
use crate::agent::links::ConnectionLink;
use crate::chat::ChatEvent;
use crate::chat::group::{GroupInvitation, GroupMemberRole, MemberIdRole};
use crate::chat::group_prefs::{FeatureEnabled, GroupFeature, GroupPreferences};
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::engine::agent_port::{AgentEvent, AgentFailure, ConnId};
use crate::engine::records::{MemberCategory, MemberStatus};
use crate::engine::tests::{T, messages};

pub fn invitation(n: u8, from: GroupMemberRole, to: GroupMemberRole) -> GroupInvitation {
    let id_role = |member_role| MemberIdRole {
        member_id: MemberId::mint().unwrap(),
        member_role,
    };
    GroupInvitation {
        from_member: id_role(from),
        from_member_key: Some("ann-key".into()),
        invited_member: id_role(to),
        conn_request: link(n),
        group_profile: profile("team"),
        business: None,
        group_link_id: None,
        group_size: Some(2),
    }
}

/// The user is invited to "team" by her friend ann: (group, ann's friend conn).
pub fn invited(t: &mut T) -> (i64, ConnId) {
    let (_, ann) = t.friend("ann");
    t.peer_says(
        &ann,
        1,
        ChatEvent::GrpInv(invitation(
            1,
            GroupMemberRole::Owner,
            GroupMemberRole::Member,
        )),
    );
    (*t.e.state.groups.keys().last().unwrap(), ann)
}

/// Joined and connected to the host: (group, host member, host conn).
pub fn joined_team(t: &mut T) -> (i64, i64, ConnId) {
    let (g, _) = invited(t);
    t.req(Request::JoinGroup(g));
    let host = t.e.state.groups[&g].host.unwrap();
    let conn = t.e.state.members[&(g, host)].conn.unwrap();
    let id = t.e.state.members[&(g, host)].member_id.clone();
    let info = conn_info(ChatEvent::GrpMemInfo {
        member_id: id,
        profile: Profile::new("ann", ""),
    });
    t.agent(AgentEvent::Info {
        conn,
        conn_info: info,
    });
    t.agent(AgentEvent::Con { conn });
    (g, host, conn)
}

#[test]
fn x_grp_inv_from_a_friend_makes_an_invited_group_with_the_friend_as_its_host() {
    let mut t = T::started();
    let (ann, ann_conn) = t.friend("ann");
    let inv = invitation(1, GroupMemberRole::Owner, GroupMemberRole::Member);
    let out = t.peer_says(&ann_conn, 1, ChatEvent::GrpInv(inv.clone()));
    let g = t.e.state.groups.values().next().unwrap().clone();
    assert_eq!(
        (g.status, &g.my_member_id, &g.my_role),
        (
            MemberStatus::Invited,
            &inv.invited_member.member_id,
            &GroupMemberRole::Member
        )
    );
    assert_eq!(g.invited_by_friend, Some(ann));
    let h = &t.e.state.members[&(g.id, g.host.unwrap())];
    assert_eq!(
        (h.category, h.status, h.friend),
        (MemberCategory::Host, MemberStatus::Invited, Some(ann))
    );
    assert_eq!(
        (h.member_id.clone(), h.role.clone()),
        (inv.from_member.member_id, GroupMemberRole::Owner)
    );
    assert_eq!(h.key.as_deref(), Some("ann-key"));
    assert_eq!(h.profile.display_name, "ann");
    assert_eq!(messages(&out)[0].chat, ChatRef::Direct(ann));
    assert_eq!(
        notices(&out),
        [ChatNotice::GroupInvitation {
            group: "team".into(),
            sent: false
        }]
    );
    let groups = out.iter().find_map(|e| match e {
        Event::Chats { groups, .. } => Some(groups.clone()),
        _ => None,
    });
    assert_eq!(groups.unwrap()[0].membership, Membership::Invited);
}

#[test]
fn a_repeated_invitation_updates_the_group_it_made_instead_of_making_another() {
    let mut t = T::started();
    let (g, ann) = invited(&mut t);
    let mut again = invitation(1, GroupMemberRole::Owner, GroupMemberRole::Admin);
    again.group_profile = profile("team two");
    t.peer_says(&ann, 2, ChatEvent::GrpInv(again.clone()));
    assert_eq!(t.e.state.groups.len(), 1);
    assert_eq!(t.e.state.members.len(), 1);
    let gr = &t.e.state.groups[&g];
    assert_eq!(
        (gr.my_role.clone(), gr.my_member_id.clone()),
        (GroupMemberRole::Admin, again.invited_member.member_id)
    );
    assert_eq!(gr.profile.display_name, "team two");
    t.peer_says(
        &ann,
        3,
        ChatEvent::GrpInv(invitation(
            2,
            GroupMemberRole::Owner,
            GroupMemberRole::Member,
        )),
    );
    assert_eq!(t.e.state.groups.len(), 2, "another link is another group");
}

#[test]
fn an_invitation_from_a_non_admin_for_a_higher_role_with_one_id_or_to_a_channel_is_dropped() {
    let mut t = T::started();
    let (_, ann) = t.friend("ann");
    let member = invitation(1, GroupMemberRole::Member, GroupMemberRole::Member);
    let higher = invitation(2, GroupMemberRole::Admin, GroupMemberRole::Owner);
    let mut same = invitation(3, GroupMemberRole::Owner, GroupMemberRole::Member);
    same.invited_member.member_id = same.from_member.member_id.clone();
    let mut channel = invitation(4, GroupMemberRole::Owner, GroupMemberRole::Member);
    channel.group_profile.public_group = Some(serde_json::json!({"groupType": "channel"}));
    for (n, inv) in [member, higher, same, channel].into_iter().enumerate() {
        let out = t.peer_says(&ann, i64::try_from(n).unwrap() + 1, ChatEvent::GrpInv(inv));
        assert!(notices(&out).is_empty());
    }
    assert!(t.e.state.groups.is_empty());
    assert_eq!(
        t.acks().len(),
        4,
        "a dropped invitation is still acknowledged"
    );
}

#[test]
fn joining_sends_x_grp_acpt_with_the_users_member_id_and_profile_then_waits_accepted() {
    let mut t = T::started();
    let (g, _) = invited(&mut t);
    let out = t.req(Request::JoinGroup(g));
    let (
        l,
        ChatEvent::GrpAcpt {
            member_id,
            member_key,
            profile,
        },
    ) = only(joined(&t))
    else {
        panic!()
    };
    assert_eq!(l, ConnectionLink::parse(&link(1)).unwrap().to_uri());
    assert_eq!(member_id, t.e.state.groups[&g].my_member_id);
    assert_eq!(
        member_key, None,
        "member keys are optional in classic groups"
    );
    let profile = profile.unwrap();
    assert_eq!(
        (profile.display_name.as_str(), profile.preferences),
        ("alice", None),
        "the user's own name"
    );
    let gr = &t.e.state.groups[&g];
    assert_eq!(gr.status, MemberStatus::Accepted);
    let h = &t.e.state.members[&(g, gr.host.unwrap())];
    assert_eq!(h.status, MemberStatus::Accepted);
    assert_eq!(t.e.state.member_of(&h.conn.unwrap()), Some((g, h.id)));
    let groups = out.iter().find_map(|e| match e {
        Event::Chats { groups, .. } => Some(groups.clone()),
        _ => None,
    });
    assert_eq!(groups.unwrap()[0].membership, Membership::Joining);
}

#[test]
fn joining_needs_an_invitation_and_happens_once() {
    let mut t = T::started();
    let (seeded, _) = t.group("club", &["carol"], GroupMemberRole::Member);
    let out = t.req(Request::JoinGroup(seeded));
    assert!(out.iter().any(|e| matches!(e, Event::Failed { .. })));
    let (g, _) = invited(&mut t);
    t.req(Request::JoinGroup(g));
    let out = t.req(Request::JoinGroup(g));
    assert!(out.iter().any(|e| matches!(e, Event::Failed { .. })));
    assert_eq!(joined(&t).len(), 1);
}

#[test]
fn a_failed_join_leaves_the_invitation_to_try_again() {
    let mut t = T::started();
    let (g, _) = invited(&mut t);
    t.e.agent.fail_next = Some(AgentFailure::Network("down".into()));
    let out = t.req(Request::JoinGroup(g));
    assert!(out.iter().any(|e| matches!(e, Event::Failed { .. })));
    let gr = &t.e.state.groups[&g];
    assert_eq!(gr.status, MemberStatus::Invited);
    assert_eq!(
        t.e.state.members[&(g, gr.host.unwrap())].status,
        MemberStatus::Invited
    );
    t.req(Request::JoinGroup(g));
    assert_eq!(t.e.state.groups[&g].status, MemberStatus::Accepted);
}

#[test]
fn on_con_with_the_host_the_user_has_joined_and_sees_who_let_them_in() {
    let mut t = T::started();
    let (g, _) = invited(&mut t);
    t.req(Request::JoinGroup(g));
    let host = t.e.state.groups[&g].host.unwrap();
    let conn = t.e.state.members[&(g, host)].conn.unwrap();
    let id = t.e.state.members[&(g, host)].member_id.clone();
    let info = conn_info(ChatEvent::GrpMemInfo {
        member_id: id,
        profile: Profile::new("ann b", ""),
    });
    t.agent(AgentEvent::Info {
        conn,
        conn_info: info,
    });
    assert_eq!(t.e.state.members[&(g, host)].profile.display_name, "ann b");
    assert!(t.e.state.members[&(g, host)].chat_v.is_some());
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(
        notices(&out),
        [
            ChatNotice::YouJoined,
            ChatNotice::MemberJoined("ann b".into())
        ]
    );
    assert_eq!(t.e.state.groups[&g].status, MemberStatus::Connected);
    assert_eq!(
        t.e.state.members[&(g, host)].status,
        MemberStatus::Connected
    );
    assert!(messages(&out).iter().all(|m| m.chat == ChatRef::Group(g)));
}

#[test]
fn without_history_the_groups_description_is_the_welcome_message() {
    let mut t = T::started();
    let (_, ann) = t.friend("ann");
    let mut inv = invitation(1, GroupMemberRole::Owner, GroupMemberRole::Member);
    inv.group_profile.description = Some("welcome!".into());
    t.peer_says(&ann, 1, ChatEvent::GrpInv(inv.clone()));
    let mut with_history = invitation(2, GroupMemberRole::Owner, GroupMemberRole::Member);
    with_history.group_profile.description = Some("hello".into());
    with_history.group_profile.group_preferences = Some(GroupPreferences {
        history: Some(GroupFeature::new(FeatureEnabled::On)),
        ..GroupPreferences::default()
    });
    t.peer_says(&ann, 2, ChatEvent::GrpInv(with_history));
    let texts: Vec<Vec<MessageBody>> =
        t.e.state
            .groups
            .keys()
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
            .map(|g| {
                t.req(Request::JoinGroup(g));
                let host = t.e.state.groups[&g].host.unwrap();
                let conn = t.e.state.members[&(g, host)].conn.unwrap();
                let out = t.agent(AgentEvent::Con { conn });
                messages(&out)
                    .into_iter()
                    .map(|m| m.body)
                    .filter(|b| matches!(b, MessageBody::Text(_)))
                    .collect()
            })
            .collect();
    assert_eq!(texts, [vec![MessageBody::Text("welcome!".into())], vec![]]);
}

#[test]
fn a_restart_while_joining_keeps_the_join() {
    let mut t = T::started();
    let (g, _) = invited(&mut t);
    t.req(Request::JoinGroup(g));
    let host = t.e.state.groups[&g].host.unwrap();
    let conn = t.e.state.members[&(g, host)].conn.unwrap();
    let mut t = t.restarted();
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(notices(&out)[0], ChatNotice::YouJoined);
    assert_eq!(t.e.state.groups[&g].status, MemberStatus::Connected);
}

#[test]
fn x_grp_inv_over_a_member_connection_needs_direct_messages_in_that_group() {
    let mut t = T::started();
    let (club, members) = t.group("club", &["ann"], GroupMemberRole::Member);
    let (ann, conn) = members[0];
    let inv = || {
        ChatEvent::GrpInv(invitation(
            1,
            GroupMemberRole::Owner,
            GroupMemberRole::Member,
        ))
    };
    t.peer_says(&conn, 1, inv());
    assert_eq!(
        t.e.state.groups.len(),
        1,
        "direct messages are off by default"
    );
    let mut c = t.e.state.groups[&club].clone();
    c.profile.group_preferences = Some(GroupPreferences {
        direct_messages: Some(GroupFeature::new(FeatureEnabled::On)),
        ..GroupPreferences::default()
    });
    t.e.save_group(c).unwrap();
    t.e.state.members.get_mut(&(club, ann)).unwrap().blocked = true;
    t.peer_says(&conn, 2, inv());
    assert_eq!(t.e.state.groups.len(), 1, "not from a blocked member");
    t.e.state.members.get_mut(&(club, ann)).unwrap().blocked = false;
    let out = t.peer_says(&conn, 3, inv());
    assert_eq!(t.e.state.groups.len(), 2);
    assert_eq!(messages(&out)[0].chat, ChatRef::Group(club));
    assert_eq!(
        notices(&out),
        [ChatNotice::GroupInvitation {
            group: "team".into(),
            sent: false
        }]
    );
    let g = *t.e.state.groups.keys().last().unwrap();
    let h = &t.e.state.members[&(g, t.e.state.groups[&g].host.unwrap())];
    assert_eq!(h.profile.display_name, "ann");
}
