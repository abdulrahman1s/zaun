//! nook's keepalive tests (smp/keepalive.unit.test.ts) with the same numbers scaled down: ping 30 -> 30 ms, timeout 15 -> 15 ms.
use std::thread::sleep;
use std::time::Duration;

use super::connection::{calm, rig};
use crate::smp::{
    Clock, Command, ConnConfig, Request, Response, SmpError, SmpEventKind, SystemClock,
};

const MS: fn(u64) -> Duration = Duration::from_millis;

fn keepalive() -> ConnConfig {
    ConnConfig {
        ping_interval: MS(30),
        command_timeout: MS(15),
        max_timeouts: 2,
        ..calm()
    }
}

fn pings(got: &[crate::smp::Incoming]) -> usize {
    got.iter().filter(|t| t.command == b"PING").count()
}

#[test]
fn silent_connection_is_closed_after_two_unanswered_pings() {
    let rig = rig(keepalive(), 23);
    let peer = rig.up();
    let mut down = None;
    while down.is_none() {
        if let SmpEventKind::Down(e) = rig.next_event() {
            down = Some(e);
        }
    }
    assert_eq!(down, Some(SmpError::Unresponsive));
    assert!(pings(&peer.recv_all(MS(50))) >= 2);
    assert!(peer.is_closed());
}

#[test]
fn healthy_connection_answers_every_ping_and_stays_up() {
    let rig = rig(keepalive(), 23);
    let peer = rig.up();
    let mut seen = 0;
    let clock = SystemClock::new();
    let end = clock.now_ms() + 600;
    while clock.now_ms() < end {
        if let Some(ts) = peer.recv(MS(10)) {
            for t in ts.iter().filter(|t| t.command == b"PING") {
                seen += 1;
                peer.reply(t, Response::Pong);
            }
        }
    }
    assert!(seen >= 10, "saw {seen} pings");
    assert!(rig.conn.is_up());
    assert!(
        rig.events
            .try_iter()
            .all(|e| !matches!(e.kind, SmpEventKind::Down(_)))
    );
}

#[test]
fn a_timeout_does_not_count_when_the_server_sent_anything_after_the_command() {
    let rig = rig(keepalive(), 23);
    let peer = rig.up();
    let clock = SystemClock::new();
    let end = clock.now_ms() + 500;
    while clock.now_ms() < end {
        peer.push(b"q", Response::End); // some push arrives while the PING is outstanding
        peer.recv_all(MS(1));
        sleep(MS(8));
    }
    assert!(rig.conn.is_up());
    assert!(
        rig.events
            .try_iter()
            .all(|e| !matches!(e.kind, SmpEventKind::Down(_)))
    );
}

#[test]
fn ping_waiters_are_not_blocked_by_a_user_command_that_timed_out() {
    let cfg = ConnConfig {
        ping_interval: Duration::ZERO,
        command_timeout: MS(20),
        max_timeouts: 3,
        ..calm()
    };
    let rig = rig(cfg, 23);
    let _peer = rig.up();
    let a = rig
        .conn
        .send(Request::new(Command::Sub, b"q".to_vec(), None))
        .unwrap();
    assert_eq!(a.recv_timeout(MS(1000)).unwrap(), Err(SmpError::Timeout));
    assert!(rig.conn.is_up());
}
