//! Offline against a real relay: stopped under a running engine, then started again. Run with the
//! `live_cli` servers plus `ZAUN_TEST_RELAY_STOP` and `ZAUN_TEST_RELAY_START`, shell commands that
//! stop the local smp-server and start it again (the second keeps running in the foreground).

use std::time::Duration;

use zaun_chat_wire::{Event, Request};

use super::Dir;
use super::live_cli::{Native, var};
use crate::engine::{Clock, SystemClock};

#[allow(
    clippy::disallowed_methods,
    reason = "a live test drives the local relay; the app starts processes only through its runners"
)]
fn shell(cmd: &str) -> std::process::Child {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .spawn()
        .unwrap()
}

fn offline_events(n: &Native) -> Vec<bool> {
    let e = n.events.iter();
    e.filter_map(|e| match e {
        Event::Offline(o) => Some(*o),
        _ => None,
    })
    .collect()
}

fn run_until(n: &mut Native, what: &str, secs: i64, f: impl Fn(&Native) -> bool) {
    let start = SystemClock.wall_ms();
    while !f(n) {
        assert!(
            SystemClock.wall_ms() - start < secs * 1000,
            "timed out waiting for {what}: {:?}",
            offline_events(n)
        );
        n.step(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "needs the live_cli servers and commands that stop and start the relay"]
fn a_relay_that_goes_away_and_comes_back_is_reported_offline_once_and_online_once() {
    let dir = Dir::new();
    let mut n = Native::start(&dir);
    n.req(Request::CreateInvite);
    run_until(&mut n, "the invitation", 30, |n| {
        n.events.iter().any(|e| matches!(e, Event::Invite(_)))
    });
    assert_eq!(offline_events(&n), Vec::<bool>::new(), "never offline yet");

    shell(&var("ZAUN_TEST_RELAY_STOP")).wait().unwrap();
    run_until(&mut n, "offline", 60, |n| offline_events(n) == [true]);
    let mut relay = shell(&var("ZAUN_TEST_RELAY_START"));
    run_until(&mut n, "online again", 120, |n| {
        offline_events(n) == [true, false]
    });
    // A little longer: nothing more is said while the relay stays up.
    let quiet = SystemClock.wall_ms();
    while SystemClock.wall_ms() - quiet < 5000 {
        n.step(Duration::from_millis(50));
    }
    assert_eq!(offline_events(&n), [true, false], "once each way");
    relay.kill().unwrap();
    relay.wait().unwrap();
}
