//! The invitation handshake against the in-memory server: full and short links, PQ off and on.
use super::support::{WAIT, connect, next_msg, rig};
use crate::agent::client::AgentEvent;
use crate::agent::testing::pump;

#[test]
fn a_full_link_connects_both_sides_with_two_skeys_and_no_lkey() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    connect(&mut a, &mut b, false, false);
    assert_eq!((r.smp.count("SKEY"), r.smp.count("LKEY")), (2, 0));
}

#[test]
fn a_short_link_is_secured_by_lkey_so_the_joiner_sends_no_skey() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    connect(&mut a, &mut b, false, true);
    assert_eq!((r.smp.count("SKEY"), r.smp.count("LKEY")), (1, 1));
}

#[test]
fn post_quantum_connections_exchange_messages_both_ways() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, true, true);
    assert!(
        a.agent
            .conn(ca)
            .unwrap()
            .ratchet
            .as_ref()
            .unwrap()
            .pq_support()
            .0
    );
    a.agent.send(&mut a.store, ca, b"to bob".to_vec()).unwrap();
    let (id, body) = next_msg(&mut [&mut a, &mut b], 1, cb);
    assert_eq!(body, b"to bob");
    b.agent.ack(&mut b.store, cb, id, None).unwrap();
    b.agent
        .send(&mut b.store, cb, b"to alice".to_vec())
        .unwrap();
    assert_eq!(next_msg(&mut [&mut a, &mut b], 0, ca).1, b"to alice");
}

#[test]
fn a_joiner_with_pq_proposes_it_to_an_inviter_without() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let ca = a
        .agent
        .create_invitation(&mut a.store, false, false)
        .unwrap();
    let full = pump(&mut [&mut a, &mut b], WAIT, |n| {
        n[0].take(|e| match e {
            AgentEvent::Invitation { full, .. } => Some(full.clone()),
            _ => None,
        })
    });
    let link = crate::agent::links::ConnectionLink::parse(&full).unwrap();
    b.agent.join(&mut b.store, &link, vec![], true).unwrap();
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        n[0].take(|e| matches!(e, AgentEvent::Conf { conn, .. } if *conn == ca).then_some(()))
    });
}

#[test]
fn allow_before_the_confirmation_is_prohibited() {
    let r = rig();
    let mut a = r.node("a");
    let ca = a
        .agent
        .create_invitation(&mut a.store, false, false)
        .unwrap();
    pump(&mut [&mut a], WAIT, |n| {
        n[0].events
            .iter()
            .any(|e| matches!(e, AgentEvent::Invitation { .. }))
            .then_some(())
    });
    assert!(a.agent.allow(&mut a.store, ca, vec![]).is_err());
    assert!(a.agent.send(&mut a.store, ca, vec![]).is_err());
}
