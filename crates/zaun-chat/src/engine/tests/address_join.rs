//! Joining someone's address (codec: the_users_own_link_and_a_known_one_connect_nothing, an_engine_error_for_a_link_is_…).

use zaun_chat_wire::model::{ChatNotice, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::{Call, T, messages, x_info};
use crate::agent::links::ConnectionLink;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::agent_port::{AgentEvent, AgentFailure};

const SHORT: &str = "https://smp6.simplex.im/a#0sPWloEa5zFH0sfc_vANe4RUUebBQSGcvTKpKRH300s";
const FULL: &str = "simplex:/contact#/?v=6-8&smp=smp%3A%2F%2FAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE%3D%40fake.test%2F-FWjHkx7I4TOSuF3q4l2S4jD2BgprZRG%23%2F%3Fv%3D1-4%26dh%3DMCowBQYDK2VuAyEAQWIOFJ7ZTMqTFfTsHuyRYNDwszkpHQfRxObZXAWAIRU%253D%26q%3Dc";

fn failed(out: &[Event]) -> (RequestKind, String) {
    match out {
        [Event::Failed { request, code, .. }] => (*request, code.clone()),
        other => panic!("{other:?}"),
    }
}

fn joins(t: &T) -> Vec<(String, Vec<u8>)> {
    t.e.agent
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::Join { link, conn_info } => Some((link.clone(), conn_info.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn connecting_to_an_address_joins_with_x_contact_and_a_request_id() {
    let mut t = T::started();
    let out = t.req(Request::Connect(format!("  {SHORT} ")));
    assert!(matches!(out.as_slice(), [Event::Chats { .. }]), "{out:?}");
    let (link, conn_info) = joins(&t).pop().unwrap();
    assert!(ConnectionLink::parse(&link).is_ok());
    assert!(
        link.starts_with("simplex:/a#0sPWloEa5zFH0sfc_vANe4RUUebBQSGcvTKpKRH300s"),
        "{link}"
    );
    let [x] = parse_chat_messages(&conn_info).unwrap().try_into().unwrap();
    let ChatEvent::Contact {
        profile,
        contact_req_id: Some(id),
        ..
    } = x.event
    else {
        panic!("not an x.contact")
    };
    assert_eq!((profile.display_name.as_str(), id.0.len()), ("alice", 16));
    assert!(x.msg_id.is_none(), "conn-info carries no message id");
}

#[test]
fn a_full_address_link_connects_too_and_each_connect_has_its_own_request_id() {
    let mut t = T::started();
    t.req(Request::Connect(FULL.into()));
    let a = joins(&t).pop().unwrap();
    assert!(a.0.starts_with("simplex:/contact#"), "{}", a.0);
    let mut t2 = T::started();
    t2.req(Request::Connect(FULL.into()));
    assert_ne!(a.1, joins(&t2).pop().unwrap().1);
}

#[test]
fn group_links_are_still_refused_as_links() {
    let mut t = T::started();
    let group_data = "&data=%7B%22groupLinkId%22%3A%22abc%22%7D";
    for link in [SHORT.replace("/a#", "/g#"), format!("{FULL}{group_data}")] {
        let out = t.req(Request::Connect(link));
        assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-005".into()));
    }
    assert!(joins(&t).is_empty());
}

#[test]
fn the_users_own_address_connects_nothing() {
    let mut t = T::started();
    t.req(Request::CreateAddress);
    let conn = t.e.state.address.creating.unwrap();
    t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: Some(SHORT.into()),
    });
    for link in [SHORT, FULL] {
        let out = t.req(Request::Connect(link.into()));
        assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-005".into()));
    }
    assert!(joins(&t).is_empty());
}

#[test]
fn an_address_already_joined_only_reads_the_list_again() {
    let mut t = T::started();
    t.req(Request::Connect(SHORT.into()));
    let out = t.req(Request::Connect(SHORT.into()));
    assert!(matches!(out.as_slice(), [Event::Chats { .. }]), "{out:?}");
    assert_eq!(joins(&t).len(), 1);
}

#[test]
fn an_address_whose_relay_does_not_answer_is_no_connection() {
    let mut t = T::started();
    t.e.agent.fail_next = Some(AgentFailure::Network("timeout".into()));
    let out = t.req(Request::Connect(SHORT.into()));
    assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-004".into()));
    assert!(t.e.state.friends.is_empty());
}

#[test]
fn an_address_that_was_deleted_is_reported_as_a_bad_link_and_can_be_removed() {
    let mut t = T::started();
    t.req(Request::Connect(SHORT.into()));
    let conn = *t.e.state.conns.keys().next().unwrap();
    let err = AgentFailure::Other("AUTH".into());
    let out = t.agent(AgentEvent::Err { conn, err });
    assert_eq!(failed(&out), (RequestKind::Connect, "ZN-CHT-005".into()));
    let id = *t.e.state.friends.keys().next().unwrap();
    t.req(Request::RemoveFriend(id));
    assert!(t.e.state.friends.is_empty());
}

#[test]
fn the_owner_accepting_gives_us_their_profile_and_we_answer_with_ours_until_ready() {
    let mut t = T::started();
    t.req(Request::Connect(SHORT.into()));
    let conn = *t.e.state.conns.keys().next().unwrap();
    let out = t.agent(AgentEvent::Conf {
        conn,
        conn_info: x_info("dana"),
    });
    let Some(Call::Allow {
        conn: to,
        conn_info,
    }) = t.e.agent.calls.last().cloned()
    else {
        panic!("{:?}", t.e.agent.calls)
    };
    assert_eq!(to, conn);
    assert!(String::from_utf8_lossy(&conn_info).contains("\"displayName\":\"alice\""));
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert_eq!(
        (friends[0].name.as_str(), friends[0].ready),
        ("dana", false)
    );
    let out = t.agent(AgentEvent::Con { conn });
    assert_eq!(
        messages(&out)[0].body,
        MessageBody::Notice(ChatNotice::Connected)
    );
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert!(friends[0].ready);
}
