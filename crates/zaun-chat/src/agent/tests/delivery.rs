//! Delivery against the in-memory server: receipts, QUOTA and A_QCONT, retries while the server is away, expiry.
use super::support::{WAIT, connect, next_msg, rig, wait_for};
use crate::agent::client::{AgentEvent, RcptStatus};
use crate::agent::error::AgentError;
use crate::agent::testing::pump;

#[test]
fn a_receipt_reaches_the_sender_once_the_recipient_acks_with_one() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    let sent = a.agent.send(&mut a.store, ca, b"hi".to_vec()).unwrap();
    wait_for(&mut [&mut a, &mut b], 0, |e| {
        (*e == AgentEvent::Sent {
            conn: ca,
            msg_id: sent,
        })
        .then_some(())
    });
    let (id, _) = next_msg(&mut [&mut a, &mut b], 1, cb);
    b.agent
        .ack(&mut b.store, cb, id, Some(b"info".to_vec()))
        .unwrap();
    let (msg, receipt) = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Rcvd {
            msg_id, receipt, ..
        } => Some((*msg_id, receipt.clone())),
        _ => None,
    });
    assert_eq!(
        (msg, receipt.status, receipt.info.as_slice()),
        (sent, RcptStatus::Ok, &b"info"[..])
    );
    assert_eq!(
        b.agent.ack(&mut b.store, cb, id, None),
        Err(AgentError::MsgNotFound)
    );
}

#[test]
fn a_full_queue_slows_sending_until_the_recipient_drains_it_and_sends_qcont() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    r.smp.lock().queues.quota = 1;
    a.agent.send(&mut a.store, ca, b"first".to_vec()).unwrap();
    let (id, _) = next_msg(&mut [&mut a, &mut b], 1, cb);
    let second = a.agent.send(&mut a.store, ca, b"second".to_vec()).unwrap();
    wait_for(&mut [&mut a, &mut b], 0, |e| {
        matches!(e, AgentEvent::Mwarn { msg_id, .. } if *msg_id == second).then_some(())
    });
    b.agent.ack(&mut b.store, cb, id, None).unwrap();
    wait_for(&mut [&mut a, &mut b], 0, |e| {
        (*e == AgentEvent::Qcont { conn: ca }).then_some(())
    });
    assert_eq!(next_msg(&mut [&mut a, &mut b], 1, cb).1, b"second");
}

#[test]
fn a_message_waits_while_the_server_is_unreachable_and_goes_when_it_returns() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    r.smp.refuse(true);
    r.smp.drop_links();
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        n[0].events
            .iter()
            .any(|e| matches!(e, AgentEvent::Down { .. }))
            .then_some(())
    });
    let sent = a.agent.send(&mut a.store, ca, b"later".to_vec()).unwrap();
    for _ in 0..20 {
        a.step(std::time::Duration::from_millis(5));
    }
    assert!(!a.events.contains(&AgentEvent::Sent {
        conn: ca,
        msg_id: sent
    }));
    r.smp.refuse(false);
    assert_eq!(next_msg(&mut [&mut a, &mut b], 1, cb).1, b"later");
}

#[test]
fn a_message_that_cannot_go_for_two_days_expires() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, _) = connect(&mut a, &mut b, false, false);
    r.smp.refuse(true);
    r.smp.drop_links();
    let sent = a.agent.send(&mut a.store, ca, b"never".to_vec()).unwrap();
    r.clock.advance(2 * 24 * 3600 * 1000 + 1);
    let err = wait_for(&mut [&mut a], 0, |e| match e {
        AgentEvent::Merr { msg_id, err, .. } if *msg_id == sent => Some(err.clone()),
        _ => None,
    });
    assert_eq!(err, AgentError::Expired);
}
