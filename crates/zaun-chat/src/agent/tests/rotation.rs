//! Following the peer to a new queue at agent v8: SKEY, the empty confirmation, the move, QEND (Agent.hs `qAddMsg`, `ICQSndSecure`).
use super::rotation_peer::Peer;
use super::rotation_support::{both_ways, move_offline, one_step, switch_of, wait_settled};
use super::support::{WAIT, connect, rig, wait_for};
use crate::agent::client::AgentEvent;
use crate::agent::error::AgentError;
use crate::agent::integrity::MsgIntegrity;
use crate::agent::message::AMessage;
use crate::agent::records::Batch;
use crate::agent::resync::SyncState;
use crate::agent::rotation::Switch;
use crate::agent::testing::pump;

#[test]
fn a_v8_switch_secures_the_new_queue_confirms_on_it_and_ends_with_qend_on_both_queues() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    let skeys = r.smp.count("SKEY");
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        switch_of(n[0], ca).is_some().then_some(())
    });
    wait_settled(&mut a, &mut b, ca);
    assert_eq!(r.smp.count("SKEY"), skeys + 1);
    p.read_old(&r, &mut b);
    assert_eq!(p.got, [AMessage::Qend(vec![p.old_addr()])]);
    p.read_new(&r, &mut b);
    assert_eq!(p.confs.len(), 1);
    assert_eq!(p.dups, 1, "the QEND on the new queue is the same message");
    both_ways(&r, &mut a, b, &p, ca);
}

#[test]
fn messages_queued_during_a_v8_switch_arrive_once_and_in_order() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    a.agent.hold_switch = true;
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        switch_of(n[0], ca).is_some().then_some(())
    });
    let sent: Vec<_> = [&b"one"[..], b"two"]
        .iter()
        .map(|m| a.agent.send(&mut a.store, ca, m.to_vec()).unwrap())
        .collect();
    for &msg_id in &sent {
        wait_for(&mut [&mut a, &mut b], 0, |e| {
            (*e == AgentEvent::Sent { conn: ca, msg_id }).then_some(())
        });
    }
    // The old queue is full, so the third waits and moves to the new queue unsent.
    r.smp.lock().queues.quota = 2;
    let third = a.agent.send(&mut a.store, ca, b"three".to_vec()).unwrap();
    wait_for(&mut [&mut a, &mut b], 0, |e| {
        matches!(e, AgentEvent::Mwarn { msg_id, .. } if *msg_id == third).then_some(())
    });
    r.smp.lock().queues.quota = 128;
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    wait_settled(&mut a, &mut b, ca);
    // The worst order for the peer: the new queue first.
    p.read_new(&r, &mut b);
    p.read_old(&r, &mut b);
    assert_eq!(p.bodies(), [&b"one"[..], b"two", b"three"]);
    assert_eq!(p.got.last(), Some(&AMessage::Qend(vec![p.old_addr()])));
    assert_eq!(p.dups, 2, "one and two went on both queues");
    let sents = a
        .events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Sent { .. }));
    assert_eq!(sents.count(), 1, "only three's Sent was still to come");
    both_ways(&r, &mut a, b, &p, ca);
}

#[test]
fn a_repeated_qadd_for_the_same_queue_is_refused_and_the_switch_still_ends() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    a.agent.hold_switch = true;
    p.qadd(&mut b);
    p.qadd(&mut b);
    let err = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Err { err, .. } => Some(err.clone()),
        _ => None,
    });
    assert_eq!(
        err,
        AgentError::Prohibited("QADD: queue address is already used in connection")
    );
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    wait_settled(&mut a, &mut b, ca);
    p.read_new(&r, &mut b);
    p.read_old(&r, &mut b);
    assert_eq!(p.confs.len(), 1);
    assert_eq!(p.got, [AMessage::Qend(vec![p.old_addr()])]);
    both_ways(&r, &mut a, b, &p, ca);
}

#[test]
/// A decrypted message resets Required (and Allowed, Agreed) to Ok before it is handled, as in
/// 7.1.4, so only a re-sync this side started still refuses a `QADD`.
fn qadd_is_refused_while_a_ratchet_resync_this_side_started_waits_and_the_chain_goes_on() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let (mut rec, _) = a.agent.editable(ca).unwrap();
    rec.sync_mut().state = SyncState::Started;
    a.agent
        .save(&mut a.store, rec, None, Batch::default())
        .unwrap();
    let p = Peer::start(&r, &mut a, &mut b, cb);
    p.qadd(&mut b);
    let err = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Err { err, .. } => Some(err.clone()),
        _ => None,
    });
    assert_eq!(err, AgentError::Prohibited("ratchet is not synchronized"));
    assert_eq!(switch_of(&a, ca), None);
    b.agent
        .send(&mut b.store, cb, b"still here".to_vec())
        .unwrap();
    let integrity = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Msg {
            body, integrity, ..
        } if body == b"still here" => Some(*integrity),
        _ => None,
    });
    assert_eq!(integrity, MsgIntegrity::Ok);
}

#[test]
fn qend_reaching_the_new_queue_first_ends_the_switch_and_its_old_queue_copy_is_not_sent() {
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
    // Still held, so only the send list goes when the server is back.
    r.smp.refuse(false);
    wait_settled(&mut a, &mut b, ca);
    p.read_old(&r, &mut b);
    assert!(p.got.is_empty(), "no QEND on the old queue");
    p.read_new(&r, &mut b);
    assert_eq!(p.got, [AMessage::Qend(vec![p.old_addr()])]);
    a.agent.hold_switch = false;
    both_ways(&r, &mut a, b, &p, ca);
}
