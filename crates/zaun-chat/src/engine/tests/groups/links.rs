//! A group's link: made, read back and deleted (codec: a_group_link_is_made_read_and_deleted, the_link_of_each_group_the_user_runs_is_read_once_and_a_missing_one_is_no_failure).

use zaun_chat_wire::{Event, Request, RequestKind};

use crate::chat::group::GroupMemberRole;
use crate::engine::agent_port::{AgentEvent, AgentFailure, ConnId};
use crate::engine::tests::{Call, T};

pub const SHORT: &str = "https://smp6.simplex.im/g#0sPWloEa5zFH0sfc_vANe4RUUebBQSGcvTKpKRH300s";
pub const FULL: &str = "simplex:/contact#/?v=6-8&smp=smp%3A%2F%2FAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE%3D%40fake.test%2F-FWjHkx7I4TOSuF3q4l2S4jD2BgprZRG%23%2F%3Fv%3D1-4%26dh%3DMCowBQYDK2VuAyEAQWIOFJ7ZTMqTFfTsHuyRYNDwszkpHQfRxObZXAWAIRU%253D%26q%3Dc";

/// The group's link made: its connection.
pub fn made(t: &mut T, group: i64) -> ConnId {
    t.req(Request::CreateGroupLink(group));
    let conn = t.e.group(group).unwrap().link_conn.unwrap();
    t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: Some(SHORT.into()),
    });
    conn
}

fn link_events(out: &[Event]) -> Vec<Option<String>> {
    out.iter()
        .filter_map(|e| match e {
            Event::GroupLink { link, .. } => Some(link.clone()),
            _ => None,
        })
        .collect()
}

fn listed_link(t: &mut T, group: i64) -> Option<String> {
    let out = t.req(Request::Refresh);
    out.iter().find_map(|e| match e {
        Event::Chats { groups, .. } => groups.iter().find(|g| g.id == group)?.link.clone(),
        _ => None,
    })
}

#[test]
fn a_group_link_is_made_with_the_groups_id_and_profile_read_and_deleted() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    assert!(link_events(&t.req(Request::CreateGroupLink(g))).is_empty());
    let Some(Call::CreateGroupAddress {
        user_data,
        client_data,
    }) = t.e.agent.calls.last().cloned()
    else {
        panic!("{:?}", t.e.agent.calls)
    };
    let data: serde_json::Value = serde_json::from_str(&client_data).unwrap();
    assert!(data["groupLinkId"].as_str().is_some_and(|i| i.len() >= 22));
    let user: serde_json::Value = serde_json::from_slice(&user_data).unwrap();
    assert_eq!(user["groupProfile"]["displayName"], "team");

    let conn = t.e.group(g).unwrap().link_conn.unwrap();
    let out = t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: Some(SHORT.into()),
    });
    assert_eq!(link_events(&out), [Some(SHORT.to_owned())]);
    assert_eq!(listed_link(&mut t, g).as_deref(), Some(SHORT));

    let out = t.req(Request::DeleteGroupLink(g));
    assert_eq!(link_events(&out), [None]);
    assert!(t.e.agent.calls.contains(&Call::Delete { conn }));
    assert!(!t.e.state.conns.contains_key(&conn));
    assert_eq!(listed_link(&mut t, g), None);
}

#[test]
fn a_group_link_comes_back_after_a_restart_and_a_missing_one_is_no_failure() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    let (other, _) = t.group("club", &["carol"], GroupMemberRole::Owner);
    made(&mut t, g);
    let mut t = t.restarted();
    assert_eq!(listed_link(&mut t, g).as_deref(), Some(SHORT));
    assert_eq!(listed_link(&mut t, other), None);
    let out = t.req(Request::DeleteGroupLink(other));
    assert_eq!(link_events(&out), [None]);
}

#[test]
fn a_link_the_group_has_is_read_back_not_made_again() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    made(&mut t, g);
    let out = t.req(Request::CreateGroupLink(g));
    assert_eq!(link_events(&out), [Some(SHORT.to_owned())]);
    let made_count =
        t.e.agent
            .calls
            .iter()
            .filter(|c| matches!(c, Call::CreateGroupAddress { .. }))
            .count();
    assert_eq!(made_count, 1);
}

#[test]
fn a_group_link_needs_an_admin() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Member);
    let out = t.req(Request::CreateGroupLink(g));
    assert!(matches!(
        out.as_slice(),
        [Event::Failed {
            request: RequestKind::GroupLink,
            ..
        }]
    ));
    assert!(
        !t.e.agent
            .calls
            .iter()
            .any(|c| matches!(c, Call::CreateGroupAddress { .. }))
    );
}

#[test]
fn an_agent_error_while_the_link_is_made_ends_the_attempt() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Admin);
    t.req(Request::CreateGroupLink(g));
    let conn = t.e.group(g).unwrap().link_conn.unwrap();
    let out = t.agent(AgentEvent::Err {
        conn,
        err: AgentFailure::Network("relay down".into()),
    });
    assert!(matches!(
        out.as_slice(),
        [Event::Failed {
            request: RequestKind::GroupLink,
            code,
            ..
        }] if code == "ZN-CHT-004"
    ));
    assert_eq!(t.e.group(g).unwrap().link_conn, None);
    t.req(Request::CreateGroupLink(g));
    assert!(t.e.group(g).unwrap().link_conn.is_some(), "asked for again");
}

#[test]
fn leaving_the_group_deletes_its_link() {
    let mut t = T::started();
    let (g, _) = t.group("team", &["bob"], GroupMemberRole::Owner);
    let conn = made(&mut t, g);
    t.req(Request::LeaveGroup(g));
    assert!(t.e.agent.calls.contains(&Call::Delete { conn }));
    assert_eq!(t.e.group(g).unwrap().link, None);
}
