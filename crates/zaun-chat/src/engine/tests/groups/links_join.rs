//! Joining by a group link: the contact connection becomes the group's (Subscriber.hs `saveConnInfo`, Commands.hs `connectViaContact`).

use zaun_chat_wire::model::{ChatNotice, Membership, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::links::{FULL, SHORT};
use super::seed::profile;
use crate::chat::group::{
    GroupAcceptance, GroupLinkInvitation, GroupLinkRejection, GroupMemberRole,
    GroupRejectionReason, MemberIdRole,
};
use crate::chat::group_profile::{GroupProfile, MemberAdmission};
use crate::chat::ids::MemberId;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage, parse_chat_messages};
use crate::engine::agent_port::{AgentEvent, ConnId};
use crate::engine::records::{MemberCategory, MemberStatus};
use crate::engine::tests::{Call, T, messages, x_info};

const GROUP_DATA: &str = "&data=%7B%22groupLinkId%22%3A%22abc%22%7D";

fn joined(t: &mut T) -> ConnId {
    t.req(Request::Connect(SHORT.into()));
    t.e.state.friends.values().next().unwrap().conn
}

fn invitation(accepted: Option<GroupAcceptance>, group: GroupProfile) -> (Vec<u8>, MemberId) {
    let me = MemberId::mint().unwrap();
    let inv = GroupLinkInvitation {
        from_member: MemberIdRole {
            member_id: MemberId::mint().unwrap(),
            member_role: GroupMemberRole::Owner,
        },
        from_member_name: "hostess".into(),
        from_member_key: None,
        invited_member: MemberIdRole {
            member_id: me.clone(),
            member_role: GroupMemberRole::Member,
        },
        group_profile: group,
        accepted,
        business: None,
        group_size: Some(5),
    };
    let msg = ChatMessage::without_id(ChatEvent::GrpLinkInv(inv));
    (msg.to_json().unwrap(), me)
}

fn conf(t: &mut T, conn: ConnId, conn_info: Vec<u8>) -> Vec<Event> {
    t.agent(AgentEvent::Conf { conn, conn_info })
}

#[test]
fn a_group_link_is_joined_with_x_contact_and_pq_off() {
    let mut t = T::started();
    for link in [SHORT.to_owned(), format!("{FULL}{GROUP_DATA}")] {
        let out = t.req(Request::Connect(link));
        assert!(matches!(out.as_slice(), [Event::Chats { .. }]), "{out:?}");
    }
    assert_eq!(t.e.agent.pq_seen, [("join", false), ("join", false)]);
    let Some(Call::Join { link, conn_info }) =
        t.e.agent
            .calls
            .iter()
            .find(|c| matches!(c, Call::Join { .. }))
            .cloned()
    else {
        panic!("no join")
    };
    assert!(link.starts_with("simplex:/g#"), "{link}");
    let [x] = parse_chat_messages(&conn_info).unwrap().try_into().unwrap();
    let ChatEvent::Contact {
        profile,
        member_key: None,
        contact_req_id: Some(id),
        ..
    } = x.event
    else {
        panic!("not an x.contact")
    };
    assert_eq!((profile.display_name.as_str(), id.0.len()), ("alice", 16));
}

#[test]
fn a_channels_link_is_still_refused() {
    let mut t = T::started();
    for link in [SHORT.replace("/g#", "/c#"), SHORT.replace("/g#", "/r#")] {
        let out = t.req(Request::Connect(link));
        assert!(matches!(
            out.as_slice(),
            [Event::Failed {
                request: RequestKind::Connect,
                code,
                ..
            }] if code == "ZN-CHT-005"
        ));
    }
    assert!(t.e.agent.pq_seen.is_empty());
}

#[test]
fn the_hosts_x_grp_link_inv_turns_the_pending_friend_into_the_group_and_is_answered_with_x_info() {
    let mut t = T::started();
    let conn = joined(&mut t);
    let (info, me) = invitation(Some(GroupAcceptance::Accepted), profile("club"));
    conf(&mut t, conn, info);

    assert!(t.e.state.friends.is_empty(), "the pending friend is gone");
    let [g] =
        t.e.state
            .groups
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
    assert_eq!(g.profile.display_name, "club");
    assert_eq!(
        (g.my_member_id.clone(), g.my_role.clone(), g.status),
        (me, GroupMemberRole::Member, MemberStatus::Accepted)
    );
    let host = t.e.member(g.id, g.host.unwrap()).unwrap();
    assert_eq!(host.profile.display_name, "hostess");
    assert_eq!(
        (host.category, host.status, host.role, host.conn),
        (
            MemberCategory::Host,
            MemberStatus::Accepted,
            GroupMemberRole::Owner,
            Some(conn)
        )
    );
    assert_eq!(t.e.state.member_of(&conn), Some((g.id, host.id)));

    let Some(Call::Allow {
        conn: to,
        conn_info,
    }) = t.e.agent.calls.last().cloned()
    else {
        panic!("{:?}", t.e.agent.calls)
    };
    assert_eq!(to, conn);
    let [x] = parse_chat_messages(&conn_info).unwrap().try_into().unwrap();
    assert!(matches!(x.event, ChatEvent::Info { profile, .. } if profile.display_name == "alice"));

    let out = t.req(Request::Refresh);
    let Some(Event::Chats {
        groups, friends, ..
    }) = out.first()
    else {
        panic!("{out:?}")
    };
    assert!((groups.len(), friends.len()) == (1, 0));
}

#[test]
fn a_group_that_reviews_its_joiners_waits_for_the_hosts_verdict() {
    let mut t = T::started();
    let conn = joined(&mut t);
    let mut p = profile("club");
    p.member_admission = Some(MemberAdmission {
        review: Some("all".into()),
    });
    let (info, me) = invitation(Some(GroupAcceptance::Accepted), p);
    conf(&mut t, conn, info);
    let g = t.e.state.groups.values().next().cloned().unwrap();
    assert_eq!(g.status, MemberStatus::PendingReview);

    let verdict = ChatEvent::GrpLinkAcpt {
        acceptance: GroupAcceptance::Accepted,
        role: GroupMemberRole::Member,
        member_id: me,
    };
    let out = t.peer_says(&conn, 1, verdict);
    assert_eq!(t.e.group(g.id).unwrap().status, MemberStatus::Connected);
    assert!(
        messages(&out)
            .iter()
            .any(|m| m.body == MessageBody::Notice(ChatNotice::YouJoined))
    );
}

#[test]
fn a_verdict_from_a_member_who_may_not_give_it_changes_nothing() {
    let mut t = T::started();
    let conn = joined(&mut t);
    let mut p = profile("club");
    p.member_admission = Some(MemberAdmission {
        review: Some("all".into()),
    });
    let (info, me) = invitation(Some(GroupAcceptance::Accepted), p);
    conf(&mut t, conn, info);
    let g = t.e.state.groups.values().next().cloned().unwrap();
    let mut host = t.e.member(g.id, g.host.unwrap()).unwrap();
    host.role = GroupMemberRole::Member;
    t.e.save_member(host).unwrap();
    let verdict = ChatEvent::GrpLinkAcpt {
        acceptance: GroupAcceptance::Accepted,
        role: GroupMemberRole::Admin,
        member_id: me,
    };
    t.peer_says(&conn, 1, verdict);
    assert_eq!(t.e.group(g.id).unwrap().status, MemberStatus::PendingReview);
}

#[test]
fn the_hosts_profile_arrives_once_after_joining_by_link() {
    let mut t = T::started();
    let conn = joined(&mut t);
    let (info, _) = invitation(Some(GroupAcceptance::Accepted), profile("club"));
    conf(&mut t, conn, info);
    let g = t.e.state.groups.values().next().cloned().unwrap();
    let host = g.host.unwrap();
    let mem = |name: &str| ChatEvent::GrpLinkMem {
        profile: Profile::new(name, ""),
        member_key: None,
    };
    let out = t.peer_says(&conn, 1, mem("Hostess Prime"));
    assert!(messages(&out).is_empty(), "the handshake shows no rename");
    assert_eq!(
        t.e.member(g.id, host).unwrap().profile.display_name,
        "Hostess Prime"
    );
    t.peer_says(&conn, 2, mem("Someone Else"));
    assert_eq!(
        t.e.member(g.id, host).unwrap().profile.display_name,
        "Hostess Prime"
    );
}

#[test]
fn a_rejected_join_leaves_a_rejected_group_and_is_still_answered() {
    let mut t = T::started();
    let conn = joined(&mut t);
    let rejection = GroupLinkRejection {
        from_member: MemberIdRole {
            member_id: MemberId::mint().unwrap(),
            member_role: GroupMemberRole::Owner,
        },
        invited_member: MemberIdRole {
            member_id: MemberId::mint().unwrap(),
            member_role: GroupMemberRole::Observer,
        },
        group_profile: profile("club"),
        rejection_reason: GroupRejectionReason::LongName,
    };
    let info = ChatMessage::without_id(ChatEvent::GrpLinkReject(rejection))
        .to_json()
        .unwrap();
    conf(&mut t, conn, info);
    let g = t.e.state.groups.values().next().cloned().unwrap();
    assert_eq!(g.status, MemberStatus::Rejected);
    assert!(matches!(t.e.agent.calls.last(), Some(Call::Allow { .. })));
    let out = t.req(Request::Refresh);
    let Some(Event::Chats { groups, .. }) = out.first() else {
        panic!("{out:?}")
    };
    assert_eq!(groups[0].membership, Membership::Left);
}

#[test]
fn a_channels_profile_in_a_link_invitation_makes_no_group() {
    let mut t = T::started();
    let conn = joined(&mut t);
    let mut p = profile("channel");
    p.public_group = Some(serde_json::json!({"groupType": "channel"}));
    let (info, _) = invitation(Some(GroupAcceptance::Accepted), p);
    conf(&mut t, conn, info);
    assert!(t.e.state.groups.is_empty());
    assert_eq!(t.e.state.friends.len(), 1);
}

#[test]
fn an_address_owners_x_info_is_not_taken_for_a_group() {
    let mut t = T::started();
    t.req(Request::Connect(SHORT.replace("/g#", "/a#")));
    let conn = t.e.state.friends.values().next().unwrap().conn;
    conf(&mut t, conn, x_info("zed"));
    assert!(t.e.state.groups.is_empty());
    assert_eq!(t.e.state.friends.values().next().unwrap().name, "zed");
}
