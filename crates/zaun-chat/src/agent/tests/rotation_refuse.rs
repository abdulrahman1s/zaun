//! Rotation messages we refuse, and a switch a second QADD replaces (Agent.hs `qAddMsg`, `qKeyMsg`).
use super::rotation_peer::Peer;
use super::rotation_support::{both_ways, create_queue, switch_of, wait_settled};
use super::support::{WAIT, connect, rig, wait_for};
use crate::agent::client::AgentEvent;
use crate::agent::error::AgentError;
use crate::agent::message::AMessage;
use crate::agent::rotation::Switch;
use crate::agent::testing::{Node, pump};
use crate::encoding::{Version, VersionRange};
use crate::smp::Response;

fn next_err(a: &mut Node, b: &mut Node) -> AgentError {
    wait_for(&mut [a, b], 0, |e| match e {
        AgentEvent::Err { err, .. } => Some(err.clone()),
        _ => None,
    })
}

#[test]
fn qadd_without_an_address_to_replace_a_foreign_one_or_no_common_version_and_qkey_are_refused() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let p = Peer::start(&r, &mut a, &mut b, cb);
    p.say(&mut b, AMessage::Qadd(vec![(p.uri(), None)]));
    let none = AgentError::Prohibited("adding queue without switching is not supported");
    assert_eq!(next_err(&mut a, &mut b), none);
    let mut uri = p.uri();
    uri.client_v_range = VersionRange::new(Version(10), Version(11)).unwrap();
    p.say(&mut b, AMessage::Qadd(vec![(uri, Some(p.old_addr()))]));
    assert_eq!(next_err(&mut a, &mut b), AgentError::Version);
    p.say(&mut b, AMessage::Qadd(vec![(p.uri(), Some(p.new_addr()))]));
    let foreign = AgentError::Prohibited("QADD: replaced queue address is not found in connection");
    assert_eq!(next_err(&mut a, &mut b), foreign);
    let key = a
        .agent
        .conn(ca)
        .unwrap()
        .rec
        .snd
        .clone()
        .unwrap()
        .auth
        .public()
        .unwrap();
    let info = crate::agent::SmpQueueInfo::from_uri(&p.uri(), Version(4));
    p.say(&mut b, AMessage::Qkey(vec![(info, key)]));
    let qkey = AgentError::Prohibited("QKEY: queue address not found in connection");
    assert_eq!(next_err(&mut a, &mut b), qkey);
    assert_eq!(switch_of(&a, ca), None);
}

#[test]
fn a_second_qadd_for_another_queue_replaces_the_switch_and_a_late_answer_for_the_first_is_ignored()
{
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, true);
    let mut p = Peer::start(&r, &mut a, &mut b, cb);
    a.agent.hold_switch = true;
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        switch_of(n[0], ca).is_some().then_some(())
    });
    let first = p.new.clone();
    p.new = create_queue(&r);
    p.qadd(&mut b);
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        matches!(switch_of(n[0], ca), Some(Switch::Moving(m)) if m.new.snd_id == p.new.snd_id)
            .then_some(())
    });
    let before = switch_of(&a, ca);
    let tag = u64::from_be_bytes(first.snd_id[..8].try_into().unwrap());
    a.agent
        .on_switch_answer(&mut a.store, ca, tag, Ok(Response::Ok))
        .unwrap();
    assert_eq!(switch_of(&a, ca), before);
    a.agent.hold_switch = false;
    a.agent.kick(&mut a.store, ca);
    wait_settled(&mut a, &mut b, ca);
    p.read_new(&r, &mut b);
    p.read_old(&r, &mut b);
    assert_eq!(p.confs.len(), 1);
    assert_eq!(p.got, [AMessage::Qend(vec![p.old_addr()])]);
    let untouched = {
        let i = r.smp.lock();
        let q = &i.queues.by_rcv[&first.rcv_id];
        q.sender_key.is_none() && q.msgs.is_empty()
    };
    assert!(untouched, "nothing went to the replaced queue");
    both_ways(&r, &mut a, b, &p, ca);
}
