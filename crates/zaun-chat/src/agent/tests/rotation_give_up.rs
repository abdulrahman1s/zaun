//! A switch given up: the new queue refuses SKEY or QTEST, or it does not end in time; we stay on the old queue (Agent.hs `ICQSndSecure`, the `AM_QTEST_` and `AM_QEND_` outcomes).
use super::rotation_peer::Peer;
use super::rotation_support::{
    as_v7, move_offline, one_step, step_of, switch_of, wait_settled, waiting_quse,
};
use super::support::{WAIT, connect, rig, wait_for};
use crate::agent::client::{AgentEvent, ConnId};
use crate::agent::error::AgentError;
use crate::agent::message::AMessage;
use crate::agent::records::pending_ids;
use crate::agent::rotation::{Step, Switch};
use crate::agent::testing::{Node, pump};
use crate::smp::{ErrorType, SmpError};

const TWO_DAYS: i64 = 2 * 24 * 3600 * 1000;

fn next_err(a: &mut Node, b: &mut Node) -> AgentError {
    wait_for(&mut [a, b], 0, |e| match e {
        AgentEvent::Err { err, .. } => Some(err.clone()),
        _ => None,
    })
}

/// Held at SKEY, with one message sent on the old queue and kept for the new one.
fn held_with_a_kept_message(a: &mut Node, b: &mut Node, p: &Peer, ca: ConnId) {
    a.agent.hold_switch = true;
    p.qadd(b);
    pump(&mut [&mut *a, &mut *b], WAIT, |n| {
        (step_of(n[0], ca) == Some(Step::Securing)).then_some(())
    });
    let kept = a.agent.send(&mut a.store, ca, b"kept".to_vec()).unwrap();
    wait_for(&mut [&mut *a, &mut *b], 0, |e| {
        (*e == AgentEvent::Sent {
            conn: ca,
            msg_id: kept,
        })
        .then_some(())
    });
    assert_eq!(pending_ids(&mut a.store, ca).unwrap(), [kept]);
}

#[test]
fn a_refused_skey_gives_the_switch_up_drops_what_it_kept_and_sending_goes_on_on_the_old_queue() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    held_with_a_kept_message(&mut a, &mut b, &p, ca);
    let rcv_id = r.smp.lock().queues.by_snd.remove(&p.new.snd_id).unwrap();
    r.smp.lock().queues.by_rcv.remove(&rcv_id);
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    let auth = AgentError::Smp(SmpError::Server(ErrorType::Auth));
    assert_eq!(next_err(&mut a, &mut b), auth);
    assert_eq!(switch_of(&a, ca), None);
    assert!(pending_ids(&mut a.store, ca).unwrap().is_empty());
    a.agent.send(&mut a.store, ca, b"after".to_vec()).unwrap();
    wait_settled(&mut a, &mut b, ca);
    p.read_old(&r, &mut b);
    assert_eq!(p.bodies(), [&b"kept"[..], b"after"]);
    assert_eq!(p.dups, 0);
}

#[test]
fn a_refused_qtest_gives_the_v7_switch_up_and_sending_goes_on_on_the_old_queue() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    as_v7(&mut a, ca);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        waiting_quse(n[0], ca).then_some(())
    });
    // QUSE without KEY: the new queue takes nothing from us.
    p.say(&mut b, AMessage::Quse(vec![(p.new_addr(), true)]));
    let two = a.agent.send(&mut a.store, ca, b"two".to_vec()).unwrap();
    let auth = AgentError::Smp(SmpError::Server(ErrorType::Auth));
    assert_eq!(next_err(&mut a, &mut b), auth);
    wait_for(&mut [&mut a, &mut b], 0, |e| {
        (*e == AgentEvent::Sent {
            conn: ca,
            msg_id: two,
        })
        .then_some(())
    });
    wait_settled(&mut a, &mut b, ca);
    assert!(pending_ids(&mut a.store, ca).unwrap().is_empty());
    a.agent.send(&mut a.store, ca, b"after".to_vec()).unwrap();
    wait_settled(&mut a, &mut b, ca);
    p.gaps = Some(Vec::new());
    p.read_old(&r, &mut b);
    assert_eq!(p.bodies(), [&b"two"[..], b"after"]);
    // QTEST took a step of the chain and never arrived; 7.1.4 leaves the same gap (`qError "QTEST: AUTH"`).
    assert_eq!(p.gaps.as_ref().map(Vec::len), Some(1));
    assert!(r.smp.lock().queues.by_rcv[&p.new.rcv_id].msgs.is_empty());
}

#[test]
fn a_switch_that_has_not_moved_in_two_days_is_given_up_with_what_it_kept() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let p = Peer::start(&r, &mut a, &mut b, cb);
    held_with_a_kept_message(&mut a, &mut b, &p, ca);
    r.clock.advance(TWO_DAYS + 1);
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    assert_eq!(next_err(&mut a, &mut b), AgentError::Expired);
    assert_eq!(switch_of(&a, ca), None);
    assert!(pending_ids(&mut a.store, ca).unwrap().is_empty());
}

#[test]
fn a_qend_copy_the_old_queue_has_not_taken_in_two_days_is_dropped() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    a.agent.hold_switch = true;
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        switch_of(n[0], ca).is_some().then_some(())
    });
    one_step(&mut a, &mut b, ca);
    one_step(&mut a, &mut b, ca);
    move_offline(&r, &mut a, &mut b, ca);
    assert!(matches!(switch_of(&a, ca), Some(Switch::EndingOld { .. })));
    r.clock.advance(TWO_DAYS + 1);
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    assert_eq!(switch_of(&a, ca), None);
    r.smp.refuse(false);
    wait_settled(&mut a, &mut b, ca);
    p.read_old(&r, &mut b);
    assert!(p.got.is_empty(), "QEND never went to the old queue");
}
