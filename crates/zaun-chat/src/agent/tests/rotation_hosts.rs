//! The peer's new queue on another server, as the CLI picks one when it can: a new SMP session that must stay open between the switch's requests.
use super::rotation_peer::Peer;
use super::rotation_support::{
    answer_qkey, as_v7, both_ways, create_queue_on, other_server, switch_of, wait_settled,
    waiting_quse,
};
use super::support::{WAIT, connect, rig};
use crate::agent::message::AMessage;
use crate::agent::testing::pump;

#[test]
fn a_v8_switch_to_a_queue_on_another_server_completes() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    p.new = create_queue_on(&r, other_server());
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        switch_of(n[0], ca).is_some().then_some(())
    });
    wait_settled(&mut a, &mut b, ca);
    p.read_new(&r, &mut b);
    p.read_old(&r, &mut b);
    assert_eq!(p.confs.len(), 1);
    assert_eq!(p.got, [AMessage::Qend(vec![p.old_addr()])]);
    let snd = a.agent.conn(ca).unwrap().rec.snd.clone().unwrap();
    assert_eq!(snd.server, other_server());
    both_ways(&r, &mut a, b, &p, ca);
}

#[test]
fn a_v7_switch_to_a_queue_on_another_server_completes() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    as_v7(&mut a, ca);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    p.new = create_queue_on(&r, other_server());
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        waiting_quse(n[0], ca).then_some(())
    });
    p.read_old(&r, &mut b);
    answer_qkey(&r.smp, &mut p, &mut b);
    wait_settled(&mut a, &mut b, ca);
    p.read_new(&r, &mut b);
    assert_eq!(p.got.last(), Some(&AMessage::Qtest(vec![p.new_addr()])));
    let snd = a.agent.conn(ca).unwrap().rec.snd.clone().unwrap();
    assert_eq!(snd.server, other_server());
    both_ways(&r, &mut a, b, &p, ca);
}
