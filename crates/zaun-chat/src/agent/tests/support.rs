//! A fake server and two agents on it, and the invitation handshake between them.
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;

use crate::agent::client::{AgentConfig, AgentEvent, ConnId};
use crate::agent::links::ConnectionLink;
use crate::agent::testing::{Dir, FakeConnector, FakeSmp, Node, TestClock, pump};
use crate::smp::{ConnConfig, SmpServer};

pub const WAIT: Duration = Duration::from_secs(10);

pub struct Rig {
    pub smp: Arc<FakeSmp>,
    pub dir: Dir,
    pub clock: Arc<TestClock>,
}

pub fn server() -> SmpServer {
    format!(
        "smp://{}@fake.test",
        crate::encoding::b64_encode(&[1; 32], true)
    )
    .parse()
    .unwrap()
}

pub fn rig() -> Rig {
    Rig {
        smp: FakeSmp::new(),
        dir: Dir::new(),
        clock: Arc::new(TestClock(AtomicI64::new(1_000_000))),
    }
}

impl Rig {
    pub fn node(&self, name: &str) -> Node {
        let mut cfg = AgentConfig::new(vec![server()]);
        cfg.smp = ConnConfig {
            ping_interval: Duration::ZERO,
            reconnect_min: Duration::from_millis(20),
            reconnect_max: Duration::from_millis(50),
            ..ConnConfig::default()
        };
        let connector = Arc::new(FakeConnector(Arc::clone(&self.smp)));
        Node::open(
            &self.dir,
            name,
            cfg,
            connector,
            Arc::clone(&self.clock) as _,
        )
    }
}

/// Runs the handshake: `a` invites, `b` joins. Returns both connection ids.
pub fn connect(a: &mut Node, b: &mut Node, pq: bool, short: bool) -> (ConnId, ConnId) {
    let ca = a.agent.create_invitation(&mut a.store, pq, short).unwrap();
    let (full, short_link) = pump(&mut [&mut *a, &mut *b], WAIT, |n| {
        n[0].take(|e| match e {
            AgentEvent::Invitation { full, short, .. } => Some((full.clone(), short.clone())),
            _ => None,
        })
    });
    let text = if short {
        short_link.expect("a short link")
    } else {
        full
    };
    let link = ConnectionLink::parse(&text).unwrap();
    let cb = b
        .agent
        .join(&mut b.store, &link, b"bob info".to_vec(), pq)
        .unwrap();
    let conf = pump(&mut [&mut *a, &mut *b], WAIT, |n| {
        n[0].take(|e| match e {
            AgentEvent::Conf { conn, conn_info } if *conn == ca => Some(conn_info.clone()),
            _ => None,
        })
    });
    assert_eq!(conf, b"bob info");
    a.agent
        .allow(&mut a.store, ca, b"alice info".to_vec())
        .unwrap();
    let info = pump(&mut [&mut *a, &mut *b], WAIT, |n| {
        n[1].take(|e| match e {
            AgentEvent::Info { conn, conn_info } if *conn == cb => Some(conn_info.clone()),
            _ => None,
        })
    });
    assert_eq!(info, b"alice info");
    pump(&mut [&mut *a, &mut *b], WAIT, |n| {
        let up = |n: &Node, c| n.events.contains(&AgentEvent::Con { conn: c });
        (up(n[0], ca) && up(n[1], cb)).then_some(())
    });
    a.events.clear();
    b.events.clear();
    (ca, cb)
}

/// The next `Msg` on `conn`: its id and body.
pub fn next_msg(
    n: &mut [&mut Node],
    who: usize,
    conn: ConnId,
) -> (crate::agent::client::AgentMsgId, Vec<u8>) {
    pump(n, WAIT, |n| {
        n[who].take(|e| match e {
            AgentEvent::Msg {
                conn: c,
                msg_id,
                body,
                ..
            } if *c == conn => Some((*msg_id, body.clone())),
            _ => None,
        })
    })
}

/// Waits for an event `f` picks on node `who`.
pub fn wait_for<T>(n: &mut [&mut Node], who: usize, f: impl Fn(&AgentEvent) -> Option<T>) -> T {
    pump(n, WAIT, |n| n[who].take(&f))
}
