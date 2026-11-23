//! A restart at each stored step of a v8 switch: it resumes from the store and ends as one without a restart.
use super::rotation_peer::Peer;
use super::rotation_support::{both_ways, move_offline, one_step, restart, step_of, wait_settled};
use super::support::{WAIT, connect, rig};
use crate::agent::message::AMessage;
use crate::agent::rotation::Step;
use crate::agent::testing::pump;

/// `steps` stored steps of a v8 switch, then a restart; with `offline`, the move as well.
fn v8_restart_after(steps: usize, offline: bool) {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    a.agent.hold_switch = true;
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        (step_of(n[0], ca) == Some(Step::Securing)).then_some(())
    });
    for _ in 0..steps {
        one_step(&mut a, &mut b, ca);
    }
    if offline {
        move_offline(&r, &mut a, &mut b, ca);
    }
    let mut a = restart(&r, a, "a");
    r.smp.refuse(false);
    wait_settled(&mut a, &mut b, ca);
    p.read_new(&r, &mut b);
    p.read_old(&r, &mut b);
    assert!(!p.confs.is_empty());
    assert!(p.confs.iter().all(|k| *k == p.confs[0]), "one e2e key");
    assert_eq!(p.got, [AMessage::Qend(vec![p.old_addr()])]);
    both_ways(&r, &mut a, b, &p, ca);
}

#[test]
fn a_v8_switch_resumes_after_a_restart_before_its_skey() {
    v8_restart_after(0, false);
}

#[test]
fn a_v8_switch_resumes_after_a_restart_before_its_confirmation() {
    v8_restart_after(1, false);
}

#[test]
fn a_v8_switch_resumes_after_a_restart_before_the_move() {
    v8_restart_after(2, false);
}

#[test]
fn a_v8_switch_resumes_after_a_restart_after_the_move_before_qend_went() {
    v8_restart_after(2, true);
}
