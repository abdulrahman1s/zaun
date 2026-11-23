//! Following the peer to a new queue below agent v8, as 7.0.x peers do: QKEY on the old queue, QUSE, QTEST alone on the new one, the move once it went (Agent.hs `qAddMsg`, `qUseMsg`, the `AM_QTEST_` outcome).
use super::rotation_peer::Peer;
use super::rotation_support::{answer_qkey, as_v7, both_ways, step_of, wait_settled, waiting_quse};
use super::support::{WAIT, connect, rig, wait_for};
use crate::agent::client::AgentEvent;
use crate::agent::error::AgentError;
use crate::agent::message::AMessage;
use crate::agent::rotation::Step;
use crate::agent::testing::pump;

#[test]
fn a_v7_switch_sends_qkey_on_the_old_queue_then_qtest_alone_on_the_new_one_and_moves_once_it_went()
{
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    as_v7(&mut a, ca);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    let skeys = r.smp.count("SKEY");
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        waiting_quse(n[0], ca).then_some(())
    });
    let one = a.agent.send(&mut a.store, ca, b"one".to_vec()).unwrap();
    wait_for(&mut [&mut a, &mut b], 0, |e| {
        (*e == AgentEvent::Sent {
            conn: ca,
            msg_id: one,
        })
        .then_some(())
    });
    p.read_old(&r, &mut b);
    a.agent.hold_switch = true;
    answer_qkey(&r.smp, &mut p, &mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        (step_of(n[0], ca) == Some(Step::Qtest)).then_some(())
    });
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    a.agent.hold_switch = true;
    // QTEST is on its way; this one goes on the old queue, and again on the new one after QTEST.
    let two = a.agent.send(&mut a.store, ca, b"two".to_vec()).unwrap();
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        (step_of(n[0], ca) == Some(Step::Ready)).then_some(())
    });
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    wait_settled(&mut a, &mut b, ca);
    // The order 7.0.x expects: QTEST first on the new queue, then what went to both.
    p.read_new(&r, &mut b);
    p.read_old(&r, &mut b);
    assert_eq!(
        r.smp.count("SKEY"),
        skeys,
        "the peer secures a v7 queue itself"
    );
    assert!(p.confs.is_empty());
    assert_eq!(p.bodies(), [&b"one"[..], b"two"]);
    assert_eq!(p.got[2], AMessage::Qtest(vec![p.new_addr()]));
    assert_eq!(p.dups, 1, "two went on both queues");
    let sents = a.events.iter().filter(|e| {
        **e == AgentEvent::Sent {
            conn: ca,
            msg_id: two,
        }
    });
    assert_eq!(sents.count(), 1);
    both_ways(&r, &mut a, b, &p, ca);
}

#[test]
fn a_repeated_quse_is_refused_before_and_after_the_move() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    as_v7(&mut a, ca);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        waiting_quse(n[0], ca).then_some(())
    });
    p.read_old(&r, &mut b);
    a.agent.hold_switch = true;
    answer_qkey(&r.smp, &mut p, &mut b);
    p.say(&mut b, AMessage::Quse(vec![(p.new_addr(), true)]));
    let err = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Err { err, .. } => Some(err.clone()),
        _ => None,
    });
    assert_eq!(
        err,
        AgentError::Prohibited("QUSE: unexpected switch status")
    );
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    wait_settled(&mut a, &mut b, ca);
    p.read_new(&r, &mut b);
    p.say(&mut b, AMessage::Quse(vec![(p.new_addr(), true)]));
    let err = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Err { err, .. } => Some(err.clone()),
        _ => None,
    });
    assert_eq!(
        err,
        AgentError::Prohibited("QUSE: switched queue address not found in connection")
    );
    both_ways(&r, &mut a, b, &p, ca);
}
