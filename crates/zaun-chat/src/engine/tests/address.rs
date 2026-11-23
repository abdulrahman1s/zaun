//! The address and its requests (codec: the_address_is_shared_as_its_short_link_…, an_address_the_engine_already_has_…).

use std::time::Duration;

use zaun_chat_wire::model::{ChatNotice, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::{Call, FakeAgent, KEY, T, messages, x_info};
use crate::chat::ids::XContactId;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::{AgentEvent, AgentFailure, ConnId};
use crate::engine::log::Log;
use crate::engine::machine::Engine;
use crate::store::Store;

const SHORT: &str = "https://smp6.simplex.im/a#0sPWloEa5zFH0sfc_vANe4RUUebBQSGcvTKpKRH300s";
const FULL: &str = "simplex:/contact#/?v=6-8&smp=smp%3A%2F%2FAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE%3D%40fake.test%2F-FWjHkx7I4TOSuF3q4l2S4jD2BgprZRG%23%2F%3Fv%3D1-4%26dh%3DMCowBQYDK2VuAyEAQWIOFJ7ZTMqTFfTsHuyRYNDwszkpHQfRxObZXAWAIRU%253D%26q%3Dc";

fn made(t: &mut T) -> ConnId {
    t.req(Request::CreateAddress);
    let conn = t.e.state.address.creating.unwrap();
    t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: Some(SHORT.into()),
    });
    conn
}

fn contact(name: &str, req_id: Option<&[u8]>) -> Vec<u8> {
    let msg = ChatMessage::without_id(ChatEvent::Contact {
        profile: Profile::new(name, ""),
        member_key: None,
        contact_req_id: req_id.map(|i| XContactId(i.to_vec())),
        welcome_msg_id: None,
        request_msg: None,
    });
    msg.to_json().unwrap()
}

fn asks(t: &mut T, conn: ConnId, inv: u8, conn_info: Vec<u8>) -> Vec<Event> {
    t.agent(AgentEvent::Req {
        conn,
        invitation: vec![inv],
        conn_info,
        pq: true,
    })
}

fn requests(out: &[Event]) -> Vec<(i64, String)> {
    let Some(Event::Chats { requests, .. }) =
        out.iter().rev().find(|e| matches!(e, Event::Chats { .. }))
    else {
        panic!("{out:?}")
    };
    requests.iter().map(|r| (r.id, r.name.clone())).collect()
}

fn rejected(t: &T) -> Vec<Vec<u8>> {
    t.e.agent
        .calls
        .iter()
        .filter_map(|c| match c {
            Call::RejectRequest { invitation } => Some(invitation.clone()),
            _ => None,
        })
        .collect()
}

fn failed(out: &[Event]) -> (RequestKind, String) {
    match out {
        [Event::Failed { request, code, .. }] => (*request, code.clone()),
        other => panic!("{other:?}"),
    }
}

/// The same store with a new engine, started.
fn restarted(t: T) -> (T, Vec<Event>) {
    let T { e, clock, dir } = t;
    drop(e);
    clock.advance(Duration::from_secs(60));
    let store = Store::open(&dir.0.join("chat.db"), KEY).unwrap();
    let mut e = Engine::new(
        store,
        FakeAgent::default(),
        Box::new(clock.clone()),
        Log::discard(),
    );
    e.start(&dir.hello("alice")).unwrap();
    let mut t = T { e, clock, dir };
    let out = t.out();
    (t, out)
}

#[test]
fn the_address_is_shared_as_its_short_link_when_made_and_when_read_back() {
    let mut t = T::started();
    t.req(Request::CreateAddress);
    let Some(Call::CreateAddress { short, user_data }) = t.e.agent.calls.last().cloned() else {
        panic!("{:?}", t.e.agent.calls)
    };
    assert!(short, "the relay is asked for a short link");
    assert!(String::from_utf8_lossy(&user_data).contains("\"displayName\":\"alice\""));
    let conn = t.e.state.address.creating.unwrap();
    let out = t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: Some(SHORT.into()),
    });
    assert_eq!(out, vec![Event::Address(Some(SHORT.into()))]);

    let (mut t, out) = restarted(t);
    assert!(matches!(out[2], Event::Chats { .. }));
    assert_eq!(out[3], Event::Address(Some(SHORT.into())), "after the list");
    assert!(
        t.e.agent
            .calls
            .iter()
            .all(|c| !matches!(c, Call::CreateAddress { .. }))
    );
    assert!(t.out().is_empty());
}

#[test]
fn an_address_without_a_short_link_is_shared_as_the_full_one() {
    let mut t = T::started();
    t.req(Request::CreateAddress);
    let conn = t.e.state.address.creating.unwrap();
    let out = t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: None,
    });
    assert_eq!(out, vec![Event::Address(Some(FULL.into()))]);
}

#[test]
fn an_address_the_engine_already_has_is_read_back_and_only_one_is_made_at_a_time() {
    let mut t = T::started();
    t.req(Request::CreateAddress);
    t.req(Request::CreateAddress);
    let makes = |t: &T| {
        t.e.agent
            .calls
            .iter()
            .filter(|c| matches!(c, Call::CreateAddress { .. }))
            .count()
    };
    assert_eq!(makes(&t), 1, "a second tap while it is made is ignored");
    let conn = t.e.state.address.creating.unwrap();
    t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: Some(SHORT.into()),
    });
    let out = t.req(Request::CreateAddress);
    assert_eq!(
        out,
        vec![Event::Address(Some(SHORT.into()))],
        "not a failure"
    );
    assert_eq!(makes(&t), 1);
}

#[test]
fn an_address_whose_relay_does_not_answer_is_no_connection_and_can_be_asked_for_again() {
    let mut t = T::started();
    t.e.agent.fail_next = Some(AgentFailure::Network("timeout".into()));
    let out = t.req(Request::CreateAddress);
    assert_eq!(failed(&out), (RequestKind::Address, "ZN-CHT-004".into()));
    assert!(t.e.state.address.creating.is_none());
    t.req(Request::CreateAddress);
    assert!(t.e.state.address.creating.is_some());
}

#[test]
fn an_address_that_fails_while_it_is_made_is_reported_and_forgotten() {
    let mut t = T::started();
    t.req(Request::CreateAddress);
    let conn = t.e.state.address.creating.unwrap();
    let err = AgentFailure::Network("down".into());
    let out = t.agent(AgentEvent::Err {
        conn,
        err: err.clone(),
    });
    assert!(out.is_empty(), "made once more first (relays tests)");
    let conn = t.e.state.address.creating.unwrap();
    let out = t.agent(AgentEvent::Err { conn, err });
    assert_eq!(failed(&out), (RequestKind::Address, "ZN-CHT-004".into()));
    assert!(t.e.state.address.creating.is_none());
    assert!(
        t.e.agent.calls.contains(&Call::Delete { conn }),
        "no queue is left behind"
    );
}

#[test]
fn an_address_made_before_a_restart_but_answered_after_it_is_adopted() {
    let mut t = T::started();
    t.req(Request::CreateAddress);
    let conn = t.e.state.address.creating.unwrap();
    let (mut t, _) = restarted(t);
    let out = t.agent(AgentEvent::Address {
        conn,
        full: FULL.into(),
        short: Some(SHORT.into()),
    });
    assert_eq!(out, vec![Event::Address(Some(SHORT.into()))]);
}

#[test]
fn deleting_the_address_ends_it_and_forgets_its_requests() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 1, contact("bob", None));
    let out = t.req(Request::DeleteAddress);
    assert_eq!(out[0], Event::Address(None));
    assert!(requests(&out).is_empty());
    assert!(
        t.e.agent.calls.contains(&Call::Delete { conn }),
        "its queue is deleted"
    );
    let (_, out) = restarted(t);
    assert!(out.iter().all(|e| !matches!(e, Event::Address(_))));
    assert!(requests(&out).is_empty());
}

#[test]
fn a_request_through_the_address_is_listed_with_the_profile_from_x_contact() {
    let mut t = T::started();
    let conn = made(&mut t);
    let out = asks(&mut t, conn, 1, contact("bob", Some(b"0123456789abcdef")));
    assert_eq!(requests(&out), vec![(1, "bob".to_owned())]);
    assert!(t.e.agent.sent().is_empty(), "nothing is sent to them");
}

#[test]
fn a_request_made_with_x_info_is_listed_too() {
    let mut t = T::started();
    let conn = made(&mut t);
    let out = asks(&mut t, conn, 1, x_info("carol"));
    assert_eq!(requests(&out), vec![(1, "carol".to_owned())]);
}

#[test]
fn a_request_that_is_not_a_contact_request_is_dropped_at_the_agent() {
    let mut t = T::started();
    let conn = made(&mut t);
    let msg = ChatMessage::new(ChatEvent::DirectDel).unwrap();
    let out = asks(&mut t, conn, 7, msg.to_json().unwrap());
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(rejected(&t), vec![vec![7]]);
    asks(&mut t, conn, 8, b"not json".to_vec());
    assert_eq!(rejected(&t).len(), 2);
}

#[test]
fn a_request_on_a_connection_that_is_not_the_address_is_dropped() {
    let mut t = T::started();
    made(&mut t);
    let out = asks(&mut t, ConnId(999), 3, contact("bob", None));
    assert!(out.is_empty());
    assert_eq!(rejected(&t), vec![vec![3]]);
}

#[test]
fn the_same_person_asking_again_is_one_request_and_the_earlier_invitation_is_dropped() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 1, contact("bob", Some(b"0123456789abcdef")));
    let out = asks(&mut t, conn, 2, contact("bobby", Some(b"0123456789abcdef")));
    assert_eq!(
        requests(&out),
        vec![(1, "bobby".to_owned())],
        "the place is kept"
    );
    assert_eq!(rejected(&t), vec![vec![1]]);
    let out = asks(&mut t, conn, 3, contact("dave", Some(b"another request!")));
    assert_eq!(requests(&out).len(), 2);
}

#[test]
fn a_person_already_accepted_asking_again_is_dropped() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 1, contact("bob", Some(b"0123456789abcdef")));
    t.req(Request::Accept(1));
    asks(&mut t, conn, 2, contact("bob", Some(b"0123456789abcdef")));
    assert!(t.e.state.address.requests.is_empty());
    assert_eq!(rejected(&t), vec![vec![2]]);
}

#[test]
fn a_person_removed_after_being_accepted_and_asking_again_is_listed() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 1, contact("bob", Some(b"0123456789abcdef")));
    t.req(Request::Accept(1));
    let friend = *t.e.state.friends.keys().next().unwrap();
    t.req(Request::RemoveFriend(friend));
    let out = asks(&mut t, conn, 2, contact("bob", Some(b"0123456789abcdef")));
    assert_eq!(requests(&out), vec![(2, "bob".to_owned())]);
    assert!(rejected(&t).is_empty());
}

#[test]
fn past_the_cap_the_oldest_request_is_rejected() {
    let mut t = T::started();
    let conn = made(&mut t);
    for i in 0..=u8::try_from(crate::engine::address::MAX_REQUESTS).unwrap() {
        asks(&mut t, conn, i, contact(&format!("p{i}"), Some(&[i; 16])));
    }
    assert_eq!(
        t.e.state.address.requests.len(),
        crate::engine::address::MAX_REQUESTS
    );
    assert_eq!(rejected(&t), vec![vec![0]]);
    assert!(!t.e.state.address.requests.contains_key(&1));
}

#[test]
fn accepting_sends_our_profile_and_the_requester_becomes_a_friend_who_is_ready_on_con() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 5, contact("bob", Some(b"0123456789abcdef")));
    let out = t.req(Request::Accept(1));
    let Some(Call::AcceptRequest {
        invitation,
        conn_info,
    }) = t.e.agent.calls.last().cloned()
    else {
        panic!("{:?}", t.e.agent.calls)
    };
    assert_eq!(invitation, vec![5]);
    assert!(String::from_utf8_lossy(&conn_info).contains("\"displayName\":\"alice\""));
    assert!(String::from_utf8_lossy(&conn_info).contains("x.info"));
    let Some(Event::Chats {
        friends, requests, ..
    }) = out.last()
    else {
        panic!("{out:?}")
    };
    assert!(requests.is_empty(), "the request is answered");
    assert_eq!((friends[0].name.as_str(), friends[0].ready), ("bob", false));
    let friend_conn = t.e.state.friends.values().next().unwrap().conn;
    t.agent(AgentEvent::Info {
        conn: friend_conn,
        conn_info: x_info("bob"),
    });
    let out = t.agent(AgentEvent::Con { conn: friend_conn });
    assert_eq!(
        messages(&out)[0].body,
        MessageBody::Notice(ChatNotice::Connected)
    );
    let Some(Event::Chats { friends, .. }) = out.last() else {
        panic!("{out:?}")
    };
    assert!(friends[0].ready);
}

#[test]
fn rejecting_forgets_the_request_and_the_requester_is_not_told() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 5, contact("bob", None));
    let out = t.req(Request::Reject(1));
    assert!(requests(&out).is_empty());
    assert_eq!(rejected(&t), vec![vec![5]]);
    assert!(t.e.agent.sent().is_empty());
    assert!(t.e.state.friends.is_empty());
    let out = t.req(Request::Accept(1));
    assert_eq!(failed(&out), (RequestKind::Answer, "ZN-CHT-006".into()));
}

#[test]
fn requests_and_the_people_already_accepted_survive_a_restart() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 1, contact("bob", Some(b"0123456789abcdef")));
    asks(&mut t, conn, 2, contact("carol", Some(b"fedcba9876543210")));
    t.req(Request::Accept(1));
    let (mut t, out) = restarted(t);
    assert_eq!(requests(&out), vec![(2, "carol".to_owned())]);
    asks(&mut t, conn, 3, contact("bob", Some(b"0123456789abcdef")));
    let pending: Vec<i64> = t.e.state.address.requests.keys().copied().collect();
    assert_eq!(pending, vec![2], "bob is a friend already");
    t.req(Request::Reject(2));
    let (_, out) = restarted(t);
    assert!(requests(&out).is_empty());
}

#[test]
fn accepting_a_request_that_the_agent_cannot_take_reports_it_and_keeps_the_request() {
    let mut t = T::started();
    let conn = made(&mut t);
    asks(&mut t, conn, 1, contact("bob", None));
    t.e.agent.fail_next = Some(AgentFailure::Other("gone".into()));
    let out = t.req(Request::Accept(1));
    assert_eq!(failed(&out), (RequestKind::Answer, "ZN-CHT-006".into()));
    assert!(t.e.state.address.requests.contains_key(&1));
    assert!(t.e.state.friends.is_empty());
}
