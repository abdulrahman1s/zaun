//! A restart at each stored step of a v7 switch: it resumes from the store and ends as one without a restart.
use super::rotation_peer::Peer;
use super::rotation_support::{
    answer_qkey, as_v7, both_ways, move_offline, one_step, restart, step_of, wait_settled,
    waiting_quse,
};
use super::support::{WAIT, connect, rig};
use crate::agent::message::AMessage;
use crate::agent::rotation::Step;
use crate::agent::testing::pump;

#[derive(PartialEq, Eq, Clone, Copy)]
enum Stop {
    BeforeQkey,
    WaitingQuse,
    BeforeQtest,
    QtestSent,
    BeforeMove,
    AfterMove,
}

fn v7_restart_at(stop: Stop) {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    as_v7(&mut a, ca);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    a.agent.hold_switch = stop == Stop::BeforeQkey;
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        step_of(n[0], ca).is_some().then_some(())
    });
    if stop == Stop::BeforeQkey {
        assert_eq!(step_of(&a, ca), Some(Step::Qkey));
        a = restart(&r, a, "a");
    }
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        waiting_quse(n[0], ca).then_some(())
    });
    if stop == Stop::WaitingQuse {
        a = restart(&r, a, "a");
    }
    p.read_old(&r, &mut b);
    a.agent.hold_switch = true;
    answer_qkey(&r.smp, &mut p, &mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        (step_of(n[0], ca) == Some(Step::Qtest)).then_some(())
    });
    match stop {
        Stop::BeforeQkey | Stop::WaitingQuse => a.agent.hold_switch = false,
        Stop::BeforeQtest => a = restart(&r, a, "a"),
        // Built and sent, its answer not yet read.
        Stop::QtestSent => {
            a.agent.hold_switch = false;
            a.agent.kick(&mut a.store, ca);
            assert!(matches!(step_of(&a, ca), Some(Step::Testing { .. })));
            a = restart(&r, a, "a");
        }
        Stop::BeforeMove => {
            one_step(&mut a, &mut b, ca);
            assert_eq!(step_of(&a, ca), Some(Step::Ready));
            a = restart(&r, a, "a");
        }
        Stop::AfterMove => {
            one_step(&mut a, &mut b, ca);
            // Sent on the old queue and kept, so that the move leaves its copy waiting.
            a.agent.send(&mut a.store, ca, b"two".to_vec()).unwrap();
            pump(&mut [&mut a, &mut b], WAIT, |n| {
                n[0].agent.conn(ca).unwrap().sends.is_empty().then_some(())
            });
            move_offline(&r, &mut a, &mut b, ca);
            a = restart(&r, a, "a");
        }
    }
    a.agent.kick(&mut a.store, ca);
    r.smp.refuse(false);
    wait_settled(&mut a, &mut b, ca);
    p.read_new(&r, &mut b);
    p.read_old(&r, &mut b);
    assert!(matches!(p.got[0], AMessage::Qkey(_)));
    assert_eq!(p.got[1], AMessage::Qtest(vec![p.new_addr()]));
    let two = (stop == Stop::AfterMove).then_some(&b"two"[..]);
    assert_eq!(p.bodies(), two.into_iter().collect::<Vec<_>>());
    both_ways(&r, &mut a, b, &p, ca);
}

#[test]
fn a_v7_switch_resumes_after_a_restart_before_its_qkey_is_queued() {
    v7_restart_at(Stop::BeforeQkey);
}

#[test]
fn a_v7_switch_resumes_after_a_restart_while_waiting_for_quse() {
    v7_restart_at(Stop::WaitingQuse);
}

#[test]
fn a_v7_switch_resumes_after_a_restart_before_qtest_is_built() {
    v7_restart_at(Stop::BeforeQtest);
}

#[test]
fn a_v7_switch_resumes_after_a_restart_while_qtest_is_on_its_way() {
    v7_restart_at(Stop::QtestSent);
}

#[test]
fn a_v7_switch_resumes_after_a_restart_before_the_move() {
    v7_restart_at(Stop::BeforeMove);
}

#[test]
fn a_v7_switch_resumes_after_a_restart_after_the_move_before_the_copies_went() {
    v7_restart_at(Stop::AfterMove);
}
