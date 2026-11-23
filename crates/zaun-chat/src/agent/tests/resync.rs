//! Ratchet re-sync between two agents on the in-memory server: a broken ratchet, both key orders, restarts midway.
use std::time::Duration;

use super::support::{Rig, WAIT, connect, next_msg, rig, wait_for};
use crate::agent::client::{AgentEvent, ConnId};
use crate::agent::error::AgentError;
use crate::agent::records::Batch;
use crate::agent::resync::SyncState;
use crate::agent::testing::{Node, pump};
use crate::ratchet::{CURRENT_E2E_VERSION, PqSupport, Ratchet, RatchetVersions, new_resync_params};

pub fn state(n: &Node, c: ConnId) -> SyncState {
    n.agent.conn(c).unwrap().rec.sync_state()
}

/// The ratchet's associated data: the sending side's `k1`, then the receiving side's. The
/// security code is its hash.
pub fn ad(n: &Node, c: ConnId) -> Vec<u8> {
    let conn = n.agent.conn(c).unwrap();
    conn.ratchet.as_ref().unwrap().associated_data().to_vec()
}

/// Replaces the stored ratchet with an unrelated one and restarts the node.
pub fn break_ratchet(r: &Rig, mut n: Node, name: &str, conn: ConnId) -> Node {
    let (_, params) = new_resync_params(CURRENT_E2E_VERSION, PqSupport(false)).unwrap();
    let versions = RatchetVersions::new(CURRENT_E2E_VERSION);
    let (fresh, _) = Ratchet::join(versions, &params, PqSupport(false)).unwrap();
    let mut b = Batch::default();
    b.ratchet(conn, &fresh).unwrap();
    b.apply(&mut n.store).unwrap();
    n.agent.close();
    drop(n);
    r.node(name)
}

/// Runs both until neither connection is re-synchronising.
pub fn settle(a: &mut Node, b: &mut Node, ca: ConnId, cb: ConnId) {
    pump(&mut [a, b], WAIT, |n| {
        let ok = state(n[0], ca) == SyncState::Ok && state(n[1], cb) == SyncState::Ok;
        ok.then_some(())
    });
}

/// A message each way, acked.
pub fn talk(a: &mut Node, b: &mut Node, ca: ConnId, cb: ConnId) {
    a.agent.send(&mut a.store, ca, b"ping".to_vec()).unwrap();
    let (id, body) = next_msg(&mut [&mut *a, &mut *b], 1, cb);
    assert_eq!(body, b"ping");
    b.agent.ack(&mut b.store, cb, id, None).unwrap();
    b.agent.send(&mut b.store, cb, b"pong".to_vec()).unwrap();
    let (id, body) = next_msg(&mut [&mut *a, &mut *b], 0, ca);
    assert_eq!(body, b"pong");
    a.agent.ack(&mut a.store, ca, id, None).unwrap();
}

fn sync_event(n: &mut [&mut Node], who: usize, conn: ConnId, want: SyncState) {
    wait_for(n, who, |e| {
        matches!(e, AgentEvent::RatchetSync { conn: c, state } if *c == conn && *state == want)
            .then_some(())
    });
}

#[test]
fn a_broken_ratchet_is_resynced_by_the_peer_and_messages_flow_both_ways() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    talk(&mut a, &mut b, ca, cb);
    let before = ad(&a, ca);
    let mut b = break_ratchet(&r, b, "b", cb);
    let acks = r.smp.count("ACK");
    a.agent.send(&mut a.store, ca, b"lost".to_vec()).unwrap();
    sync_event(&mut [&mut a, &mut b], 1, cb, SyncState::Required);
    // The failed message is acked, so the server does not deliver it again.
    pump(&mut [&mut a, &mut b], WAIT, |_| {
        (r.smp.count("ACK") > acks).then_some(())
    });
    assert!(
        b.events
            .iter()
            .any(|e| matches!(e, AgentEvent::Err { conn, .. } if *conn == cb)),
        "the failure is reported"
    );
    assert!(matches!(
        b.agent.send(&mut b.store, cb, b"no".to_vec()),
        Err(AgentError::Prohibited(_))
    ));
    let sends = r.smp.count("SEND");
    a.agent.synchronize_ratchet(&mut a.store, ca, true).unwrap();
    sync_event(&mut [&mut a, &mut b], 1, cb, SyncState::Agreed);
    sync_event(&mut [&mut a, &mut b], 0, ca, SyncState::Agreed);
    settle(&mut a, &mut b, ca, cb);
    for _ in 0..20 {
        a.step(Duration::from_millis(2));
        b.step(Duration::from_millis(2));
    }
    for (n, c) in [(&a, ca), (&b, cb)] {
        let ok = AgentEvent::RatchetSync {
            conn: c,
            state: SyncState::Ok,
        };
        assert!(
            n.events.contains(&ok),
            "the first decrypted message ends the re-sync"
        );
    }
    // Two keys and one EREADY each way: no answer is answered again.
    assert_eq!(r.smp.count("SEND") - sends, 4);
    assert!(!b.events.iter().any(|e| matches!(e, AgentEvent::Msg { .. })));
    talk(&mut a, &mut b, ca, cb);
    assert_eq!(ad(&a, ca), ad(&b, cb));
    assert_ne!(ad(&a, ca), before);
}

#[test]
fn either_side_of_the_key_hash_order_sends_eready_and_both_converge() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    let mut seen = [false; 2];
    for _ in 0..24 {
        a.agent.synchronize_ratchet(&mut a.store, ca, true).unwrap();
        let keys = a.agent.conn(ca).unwrap().rec.sync.clone().unwrap().keys;
        let k1 = keys.unwrap().pk1.public().unwrap();
        settle(&mut a, &mut b, ca, cb);
        talk(&mut a, &mut b, ca, cb);
        let a_sends_first = ad(&a, ca).starts_with(k1.as_bytes());
        seen[usize::from(a_sends_first)] = true;
        if seen == [true, true] {
            return;
        }
    }
    panic!("one key order never came up: {seen:?}");
}

#[test]
fn a_resync_survives_a_restart_of_either_side_midway() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    let before = ad(&a, ca);
    a.agent.synchronize_ratchet(&mut a.store, ca, true).unwrap();
    a.agent.close();
    drop(a);
    let mut a = r.node("a");
    assert_eq!(state(&a, ca), SyncState::Started);
    sync_event(&mut [&mut a, &mut b], 1, cb, SyncState::Agreed);
    b.agent.close();
    drop(b);
    let mut b = r.node("b");
    settle(&mut a, &mut b, ca, cb);
    talk(&mut a, &mut b, ca, cb);
    assert_eq!(ad(&a, ca), ad(&b, cb));
    assert_ne!(ad(&a, ca), before);
}

#[test]
fn sending_waits_for_the_resync_to_finish() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    a.agent.synchronize_ratchet(&mut a.store, ca, true).unwrap();
    assert!(matches!(
        a.agent.send(&mut a.store, ca, b"early".to_vec()),
        Err(AgentError::Prohibited(_))
    ));
    assert!(matches!(
        b.agent.synchronize_ratchet(&mut b.store, cb, false),
        Err(AgentError::Prohibited(_))
    ));
    settle(&mut a, &mut b, ca, cb);
    talk(&mut a, &mut b, ca, cb);
}

#[test]
fn a_post_quantum_connection_resyncs_with_kem_keys_and_keeps_talking() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, true, false);
    talk(&mut a, &mut b, ca, cb);
    a.agent.synchronize_ratchet(&mut a.store, ca, true).unwrap();
    let keys = a.agent.conn(ca).unwrap().rec.sync.clone().unwrap().keys;
    assert!(
        keys.unwrap().kem.is_some(),
        "the new keys propose a KEM key"
    );
    settle(&mut a, &mut b, ca, cb);
    for _ in 0..3 {
        talk(&mut a, &mut b, ca, cb);
    }
    assert_eq!(ad(&a, ca), ad(&b, cb));
}

#[test]
fn the_side_left_in_required_repairs_the_connection_without_force() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    talk(&mut a, &mut b, ca, cb);
    let mut b = break_ratchet(&r, b, "b", cb);
    a.agent.send(&mut a.store, ca, b"lost".to_vec()).unwrap();
    sync_event(&mut [&mut a, &mut b], 1, cb, SyncState::Required);
    // What the engine does on that event: a plain re-sync, which `Required` allows.
    b.agent
        .synchronize_ratchet(&mut b.store, cb, false)
        .unwrap();
    assert_eq!(state(&b, cb), SyncState::Started);
    settle(&mut a, &mut b, ca, cb);
    talk(&mut a, &mut b, ca, cb);
    assert_eq!(ad(&a, ca), ad(&b, cb));
}

#[test]
fn both_sides_repairing_at_once_still_agree_on_one_ratchet() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    talk(&mut a, &mut b, ca, cb);
    let mut a = break_ratchet(&r, a, "a", ca);
    let mut b = break_ratchet(&r, b, "b", cb);
    a.agent.send(&mut a.store, ca, b"lost".to_vec()).unwrap();
    b.agent.send(&mut b.store, cb, b"lost".to_vec()).unwrap();
    sync_event(&mut [&mut a, &mut b], 0, ca, SyncState::Required);
    sync_event(&mut [&mut a, &mut b], 1, cb, SyncState::Required);
    // Each engine starts its own before it sees the other's key.
    a.agent
        .synchronize_ratchet(&mut a.store, ca, false)
        .unwrap();
    b.agent
        .synchronize_ratchet(&mut b.store, cb, false)
        .unwrap();
    settle(&mut a, &mut b, ca, cb);
    talk(&mut a, &mut b, ca, cb);
    assert_eq!(ad(&a, ca), ad(&b, cb));
}
