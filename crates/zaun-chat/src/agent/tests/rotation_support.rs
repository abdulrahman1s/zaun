//! What the rotation tests share: the peer's new queue, our switch's state, restarts and stepping, the check that messages go both ways after.
use super::rotation_peer::Peer;
use super::support::{Rig, WAIT, next_msg, server};
use crate::agent::client::ConnId;
use crate::agent::message::{AMessage, SndQAddr};
use crate::agent::records::{Batch, RcvQueue};
use crate::agent::rotation::{Step, Switch};
use crate::agent::testing::{FakeSmp, Node, new_queue, pump};
use crate::agent::version::CURRENT_CLIENT;
use crate::crypto::{Alg, KeyPair, random_vec};
use crate::encoding::Version;
use crate::smp::SmpServer;

pub fn addr(rq: &RcvQueue) -> SndQAddr {
    SndQAddr {
        server: rq.server.clone(),
        sender_id: rq.snd_id.clone(),
    }
}

/// A queue as NEW makes it, with no subscriber yet.
pub fn create_queue(r: &Rig) -> RcvQueue {
    create_queue_on(r, server())
}

/// Another host of the in-memory server: a new SMP session, as a queue on another server needs.
pub fn other_server() -> SmpServer {
    let key = crate::encoding::b64_encode(&[1; 32], true);
    format!("smp://{key}@other.test").parse().unwrap()
}

pub fn create_queue_on(r: &Rig, server: SmpServer) -> RcvQueue {
    let x = || KeyPair::generate(Alg::X25519).unwrap();
    let (auth, dh, e2e) = (KeyPair::generate(Alg::Ed25519).unwrap(), x(), x());
    let snd_id = random_vec(24).unwrap();
    let q = new_queue(auth.public, dh.public, snd_id.clone());
    let rq = RcvQueue {
        server,
        rcv_id: q.rcv_id.clone(),
        snd_id: snd_id.clone(),
        auth: auth.private,
        dh_priv: dh.private,
        server_dh: Some(q.server_dh.public.clone()),
        e2e_priv: e2e.private,
        peer_e2e: None,
        client_version: CURRENT_CLIENT,
    };
    let mut i = r.smp.lock();
    i.queues.by_snd.insert(snd_id, q.rcv_id.clone());
    i.queues.by_rcv.insert(q.rcv_id.clone(), q);
    rq
}

pub fn switch_of(a: &Node, ca: ConnId) -> Option<Switch> {
    a.agent.conn(ca).unwrap().rec.switch.clone()
}

pub fn step_of(a: &Node, ca: ConnId) -> Option<Step> {
    match switch_of(a, ca) {
        Some(Switch::Moving(m)) => Some(m.step),
        Some(Switch::EndingOld { .. }) | None => None,
    }
}

/// No switch left and nothing waiting to be sent.
pub fn wait_settled(a: &mut Node, b: &mut Node, ca: ConnId) {
    pump(&mut [a, b], WAIT, |n| {
        let c = n[0].agent.conn(ca).unwrap();
        (c.rec.switch.is_none() && c.sends.is_empty() && !c.send.in_flight).then_some(())
    });
}

pub fn restart(r: &Rig, mut n: Node, name: &str) -> Node {
    n.agent.close();
    drop(n);
    r.node(name)
}

/// Lets the switch take one stored step, then holds it again.
pub fn one_step(a: &mut Node, b: &mut Node, ca: ConnId) {
    let before = switch_of(a, ca);
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    a.agent.hold_switch = true;
    pump(&mut [a, b], WAIT, |n| {
        let c = n[0].agent.conn(ca).unwrap();
        (switch_of(n[0], ca) != before && !c.step.in_flight).then_some(())
    });
}

/// The move with the network down, so that what follows it is still waiting when the agent stops.
pub fn move_offline(r: &Rig, a: &mut Node, b: &mut Node, ca: ConnId) {
    r.smp.refuse(true);
    r.smp.drop_links();
    one_step(a, b, ca);
    assert_eq!(step_of(a, ca), None);
    assert!(!a.agent.conn(ca).unwrap().sends.is_empty());
}

/// After the switch: the peer reads its new queue as its own, and messages go both ways.
pub fn both_ways(r: &Rig, a: &mut Node, b: Node, p: &Peer, ca: ConnId) -> Node {
    let mut b = b;
    p.adopt(&mut b);
    let mut b = restart(r, b, "b");
    a.agent.send(&mut a.store, ca, b"after".to_vec()).unwrap();
    let (id, body) = next_msg(&mut [&mut *a, &mut b], 1, p.cb);
    assert_eq!(body, b"after");
    b.agent.ack(&mut b.store, p.cb, id, None).unwrap();
    b.agent.send(&mut b.store, p.cb, b"back".to_vec()).unwrap();
    assert_eq!(next_msg(&mut [&mut *a, &mut b], 0, ca).1, b"back");
    let old = &r.smp.lock().queues.by_rcv[&p.old.rcv_id];
    assert!(old.msgs.is_empty(), "nothing more goes to the old queue");
    b
}

/// The agent version a 7.0.x peer agrees on.
pub fn as_v7(a: &mut Node, ca: ConnId) {
    let (mut rec, _) = a.agent.editable(ca).unwrap();
    rec.agent_version = Version(7);
    a.agent
        .save(&mut a.store, rec, None, Batch::default())
        .unwrap();
}

/// QKEY is queued and has gone: the switch waits for QUSE.
pub fn waiting_quse(a: &Node, ca: ConnId) -> bool {
    let c = a.agent.conn(ca).unwrap();
    step_of(a, ca) == Some(Step::WaitingQuse) && c.sends.is_empty() && !c.send.in_flight
}

/// The peer reads QKEY, secures its new queue with the key in it (KEY), and answers QUSE.
pub fn answer_qkey(smp: &FakeSmp, p: &mut Peer, b: &mut Node) {
    let Some(AMessage::Qkey(qs)) = p.got.iter().find(|m| matches!(m, AMessage::Qkey(_))) else {
        panic!("no QKEY in {:?}", p.got);
    };
    let (info, key) = qs[0].clone();
    assert_eq!(info.address.sender_id, p.new.snd_id);
    p.new.peer_e2e = Some(info.address.dh_public_key);
    smp.lock().queues.secure(&p.new.snd_id, key);
    p.say(b, AMessage::Quse(vec![(p.new_addr(), true)]));
}
