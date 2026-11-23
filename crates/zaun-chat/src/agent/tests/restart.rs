//! Restart from the store: subscriptions, queued sends and unacked messages come back; delete forgets everything.
use std::time::Duration;

use super::support::{WAIT, connect, next_msg, rig};
use crate::agent::client::AgentEvent;
use crate::agent::error::AgentError;
use crate::agent::records::{NS, conn_key};
use crate::agent::testing::pump;

#[test]
fn a_message_queued_before_a_restart_is_sent_after_it() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    r.smp.refuse(true);
    r.smp.drop_links();
    b.agent.send(&mut b.store, cb, b"queued".to_vec()).unwrap();
    b.agent.close();
    drop(b);
    let mut b = r.node("b");
    r.smp.refuse(false);
    assert_eq!(next_msg(&mut [&mut a, &mut b], 0, ca).1, b"queued");
}

#[test]
fn an_unacked_message_is_handed_out_again_after_a_restart() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    a.agent.send(&mut a.store, ca, b"keep me".to_vec()).unwrap();
    let first = next_msg(&mut [&mut a, &mut b], 1, cb);
    b.agent.close();
    drop(b);
    let mut b = r.node("b");
    let again = next_msg(&mut [&mut a, &mut b], 1, cb);
    assert_eq!(again, first);
    b.agent.ack(&mut b.store, cb, again.0, None).unwrap();
    a.agent.send(&mut a.store, ca, b"next".to_vec()).unwrap();
    assert_eq!(next_msg(&mut [&mut a, &mut b], 1, cb).1, b"next");
}

#[test]
fn a_message_redelivered_after_its_ack_is_acked_without_a_second_msg() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    a.agent.send(&mut a.store, ca, b"once".to_vec()).unwrap();
    let (id, _) = next_msg(&mut [&mut a, &mut b], 1, cb);
    r.smp.refuse(true);
    r.smp.drop_links();
    pump(&mut [&mut b], WAIT, |n| {
        n[0].events
            .iter()
            .any(|e| matches!(e, AgentEvent::Down { .. }))
            .then_some(())
    });
    b.agent.ack(&mut b.store, cb, id, None).unwrap(); // the ACK cannot reach the server
    r.smp.refuse(false);
    let acks = r.smp.count("ACK");
    pump(&mut [&mut a, &mut b], WAIT, |_| {
        (r.smp.count("ACK") > acks).then_some(())
    });
    for _ in 0..20 {
        b.step(Duration::from_millis(5));
    }
    assert!(!b.events.iter().any(|e| matches!(e, AgentEvent::Msg { .. })));
    a.agent.send(&mut a.store, ca, b"after".to_vec()).unwrap();
    assert_eq!(next_msg(&mut [&mut a, &mut b], 1, cb).1, b"after");
}

#[test]
fn delete_forgets_the_connection_and_deletes_its_queue() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, _) = connect(&mut a, &mut b, false, false);
    a.agent.send(&mut a.store, ca, b"x".to_vec()).unwrap();
    a.agent.delete(&mut a.store, ca).unwrap();
    pump(&mut [&mut a, &mut b], WAIT, |_| {
        (r.smp.count("DEL") == 1).then_some(())
    });
    assert_eq!(a.store.get(NS, &conn_key(ca)).unwrap(), None);
    assert!(a.store.keys_with_prefix(NS, b"").unwrap().is_empty());
    assert_eq!(
        a.agent.send(&mut a.store, ca, vec![]),
        Err(AgentError::ConnNotFound)
    );
    drop(a);
    let a = r.node("a");
    assert!(a.agent.conn(ca).is_err());
}
