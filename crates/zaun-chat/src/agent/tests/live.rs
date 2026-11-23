//! Two native agents through a real smp-server. Start a local smp-server and run
//! `ZAUN_TEST_SMP=smp://<fingerprint>[:<password>]@127.0.0.1:<port> cargo test -p zaun-chat --lib agent::tests::live -- --ignored`.
use std::sync::Arc;
use std::time::Duration;

use super::support::{connect, next_msg, wait_for};
use crate::agent::client::{AgentConfig, AgentEvent, RcptStatus, SystemWallClock};
use crate::agent::testing::{Dir, Node};
use crate::smp::{SmpServer, TlsConnector};

pub fn live_server() -> SmpServer {
    let Ok(addr) = std::env::var("ZAUN_TEST_SMP") else {
        panic!("ZAUN_TEST_SMP is not set")
    };
    addr.parse().unwrap()
}

pub fn live_node(dir: &Dir, name: &str) -> Node {
    let mut cfg = AgentConfig::new(vec![live_server()]);
    cfg.smp.reconnect_min = Duration::from_millis(200);
    Node::open(
        dir,
        name,
        cfg,
        Arc::new(TlsConnector),
        Arc::new(SystemWallClock),
    )
}

#[test]
#[ignore = "needs a local smp-server; set ZAUN_TEST_SMP"]
fn two_native_agents_connect_both_ways_exchange_receipts_and_survive_a_restart() {
    let dir = Dir::new();
    let (mut a, mut b) = (live_node(&dir, "a"), live_node(&dir, "b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    let (ba, ab) = connect(&mut b, &mut a, true, true);

    a.agent
        .send(&mut a.store, ca, b"hello bob".to_vec())
        .unwrap();
    let (id, body) = next_msg(&mut [&mut a, &mut b], 1, cb);
    assert_eq!(body, b"hello bob");
    b.agent.ack(&mut b.store, cb, id, Some(Vec::new())).unwrap();
    let rcvd = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Rcvd { conn, receipt, .. } if *conn == ca => Some(receipt.status),
        _ => None,
    });
    assert_eq!(rcvd, RcptStatus::Ok);

    b.agent
        .send(&mut b.store, ba, b"over the pq link".to_vec())
        .unwrap();
    let (id, body) = next_msg(&mut [&mut a, &mut b], 0, ab);
    assert_eq!(body, b"over the pq link");
    a.agent.ack(&mut a.store, ab, id, None).unwrap();

    b.agent.close();
    drop(b);
    a.agent
        .send(&mut a.store, ca, b"while you were away".to_vec())
        .unwrap();
    let mut b = live_node(&dir, "b");
    let (id, body) = next_msg(&mut [&mut a, &mut b], 1, cb);
    assert_eq!(body, b"while you were away");
    b.agent.ack(&mut b.store, cb, id, None).unwrap();
    b.agent
        .send(&mut b.store, ba, b"back again".to_vec())
        .unwrap();
    assert_eq!(next_msg(&mut [&mut a, &mut b], 0, ab).1, b"back again");
}
