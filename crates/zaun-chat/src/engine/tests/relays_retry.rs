//! A request that reached no relay is made once more after a check (codec: a_connection_whose_relay_times_out_…, checks_are_at_most_every_few_minutes_…).

use zaun_chat_wire::{Event, Request, RequestKind};

use super::Call;
use super::relays_fake::{DEAD, Fake, LIVE, started};
use crate::engine::agent_port::{AgentEvent, AgentFailure, ConnId};

fn network() -> AgentFailure {
    AgentFailure::Network("BROKER NETWORK".into())
}

fn count(t: &super::T, call: &Call) -> usize {
    t.e.agent.calls.iter().filter(|c| *c == call).count()
}

fn address_calls(t: &super::T) -> usize {
    t.e.agent
        .calls
        .iter()
        .filter(|c| matches!(c, Call::CreateAddress { .. }))
        .count()
}

fn failed(out: &[Event], request: RequestKind) -> bool {
    out.iter()
        .any(|e| matches!(e, Event::Failed { request: r, code, .. } if *r == request && code == "ZN-CHT-004"))
}

/// Both relays answered the check that runs at the start.
fn settled(t: &mut super::T, fake: &Fake) {
    t.relay_answers(fake, LIVE, true);
    t.relay_answers(fake, DEAD, true);
}

fn address_fails(t: &mut super::T) -> (ConnId, Vec<Event>) {
    let conn = t.e.state.address.creating.unwrap();
    let out = t.agent(AgentEvent::Err {
        conn,
        err: network(),
    });
    (conn, out)
}

#[test]
fn an_address_whose_relay_times_out_is_made_again_after_the_check() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    settled(&mut t, &fake);
    t.relay_time(181);
    t.req(Request::CreateAddress);
    let (first, out) = address_fails(&mut t);
    assert!(out.is_empty(), "nothing is reported yet: {out:?}");
    assert_eq!(address_calls(&t), 1, "waits for the check");
    assert_eq!(count(&t, &Call::Delete { conn: first }), 1);
    // A second tap while it waits is ignored.
    t.req(Request::CreateAddress);
    assert_eq!(address_calls(&t), 1);
    t.relay_answers(&fake, LIVE, true);
    t.relay_answers(&fake, DEAD, true);
    assert_eq!(address_calls(&t), 2, "made again once the check is in");
}

#[test]
fn a_failure_soon_after_a_check_is_just_sent_again_and_the_second_failure_is_reported() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    settled(&mut t, &fake);
    let checks = fake.asked().len();
    t.relay_time(60);
    t.req(Request::CreateAddress);
    let (_, out) = address_fails(&mut t);
    assert!(out.is_empty());
    assert_eq!(
        fake.asked().len(),
        checks,
        "checked a minute ago: no new check"
    );
    assert_eq!(address_calls(&t), 2, "sent again at once");
    let (_, out) = address_fails(&mut t);
    assert!(failed(&out, RequestKind::Address), "{out:?}");
    assert_eq!(address_calls(&t), 2, "not a third time");
}

#[test]
fn checks_are_at_most_every_few_minutes() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    settled(&mut t, &fake);
    t.relay_time(181);
    t.req(Request::CreateAddress);
    address_fails(&mut t);
    assert_eq!(fake.asked().len(), 4, "a new check before the retry");
}

#[test]
fn an_invitation_whose_relay_times_out_is_made_again_after_the_check() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    settled(&mut t, &fake);
    t.relay_time(181);
    t.req(Request::CreateInvite);
    let first = t.e.state.inviting.unwrap();
    let out = t.agent(AgentEvent::Err {
        conn: first,
        err: network(),
    });
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(count(&t, &Call::CreateInvitation), 1);
    t.relay_answers(&fake, LIVE, true);
    t.relay_answers(&fake, DEAD, true);
    assert_eq!(count(&t, &Call::CreateInvitation), 2);
    let second = t.e.state.inviting.unwrap();
    assert_ne!(first, second);
    let out = t.agent(AgentEvent::Err {
        conn: second,
        err: network(),
    });
    assert!(failed(&out, RequestKind::CreateInvite), "{out:?}");
}

#[test]
fn a_failure_that_is_not_the_network_is_reported_at_once() {
    let (mut t, fake) = started(&[LIVE, DEAD]);
    settled(&mut t, &fake);
    t.relay_time(181);
    t.req(Request::CreateAddress);
    let conn = t.e.state.address.creating.unwrap();
    let out = t.agent(AgentEvent::Err {
        conn,
        err: AgentFailure::Other("refused".into()),
    });
    assert!(out.iter().any(|e| matches!(
        e,
        Event::Failed {
            request: RequestKind::Address,
            ..
        }
    )));
    assert_eq!(fake.asked().len(), 2, "no check for it");
}

#[test]
fn an_invitation_waiting_on_a_relay_a_check_disabled_is_made_on_another() {
    let (mut t, fake) = started(&[DEAD, LIVE]);
    t.req(Request::CreateInvite);
    let first = t.e.state.inviting.unwrap();
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    assert_eq!(count(&t, &Call::Delete { conn: first }), 1);
    assert_eq!(count(&t, &Call::CreateInvitation), 2);
    assert_ne!(t.e.state.inviting, Some(first));
}

#[test]
fn an_address_waiting_on_a_relay_a_check_disabled_is_made_on_another() {
    let (mut t, fake) = started(&[DEAD, LIVE]);
    t.req(Request::CreateAddress);
    let first = t.e.state.address.creating.unwrap();
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    assert_eq!(count(&t, &Call::Delete { conn: first }), 1);
    assert_eq!(address_calls(&t), 2);
}

#[test]
fn an_invitation_that_already_has_its_link_is_left_when_a_relay_is_disabled() {
    let (mut t, fake) = started(&[DEAD, LIVE]);
    t.req(Request::CreateInvite);
    let conn = t.e.state.inviting.unwrap();
    t.agent(AgentEvent::Invitation {
        conn,
        full: "simplex:/invitation#/?v=2-7&x=1".into(),
        short: None,
    });
    t.relay_answers(&fake, DEAD, false);
    t.relay_answers(&fake, LIVE, true);
    assert_eq!(count(&t, &Call::CreateInvitation), 1);
}
