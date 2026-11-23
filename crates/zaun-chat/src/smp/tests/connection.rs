use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, bounded};

use super::fake::{FakeConnector, Peer};
use super::support::*;
use crate::crypto::{Alg, CbNonce, Signature, cb_verify};
use crate::smp::{
    Answer, Command, ConnConfig, ErrorType, Request, Response, SmpConnection, SmpError, SmpEvent,
    SmpEventKind, SmpServer, SystemClock, authorized,
};

pub const WAIT: Duration = Duration::from_secs(3);

pub fn server() -> SmpServer {
    format!(
        "smp://{}@fake.test",
        crate::encoding::b64_encode(&[1; 32], true)
    )
    .parse()
    .unwrap()
}

/// Quiet settings: no keepalive, long timeouts, short back-off.
pub fn calm() -> ConnConfig {
    ConnConfig {
        ping_interval: Duration::ZERO,
        command_timeout: Duration::from_secs(10),
        reconnect_min: Duration::from_millis(20),
        reconnect_max: Duration::from_millis(40),
        ..ConnConfig::default()
    }
}

pub struct Rig {
    pub conn: SmpConnection,
    pub events: Receiver<SmpEvent>,
    pub peers: Receiver<Peer>,
}

pub fn rig(cfg: ConnConfig, version: u16) -> Rig {
    let (connector, peers) = FakeConnector::new(version);
    let (tx, events) = bounded(256);
    let conn = SmpConnection::open_with(
        server(),
        cfg,
        Arc::clone(&connector) as _,
        Arc::new(SystemClock::new()),
        tx,
    )
    .unwrap();
    Rig {
        conn,
        events,
        peers,
    }
}

impl Rig {
    pub fn next_event(&self) -> SmpEventKind {
        self.events.recv_timeout(WAIT).expect("an event").kind
    }

    /// Skips events until `Up` and returns the peer of that session.
    pub fn up(&self) -> Peer {
        loop {
            if matches!(self.next_event(), SmpEventKind::Up(_)) {
                return self.peers.recv_timeout(WAIT).expect("a peer");
            }
        }
    }
}

pub fn sub(rig: &Rig, entity: &[u8]) -> Receiver<Answer> {
    rig.conn
        .send(Request::new(
            Command::Sub,
            entity.to_vec(),
            Some(private(Alg::Ed25519)),
        ))
        .unwrap()
}

#[test]
fn command_is_answered_by_matching_its_corr_id() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let (a, b) = (sub(&rig, b"q1"), sub(&rig, b"q2"));
    let got = peer.recv_all(Duration::from_millis(200));
    assert_eq!(got.len(), 2);
    for t in got.iter().rev() {
        peer.reply(
            t,
            if t.entity_id == b"q1" {
                Response::Sok(None)
            } else {
                Response::Ok
            },
        );
    }
    assert_eq!(a.recv_timeout(WAIT).unwrap(), Ok(Response::Sok(None)));
    assert_eq!(b.recv_timeout(WAIT).unwrap(), Ok(Response::Ok));
}

#[test]
fn server_err_reaches_the_caller_as_an_error() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let a = sub(&rig, b"q");
    let t = peer.recv(WAIT).unwrap().remove(0);
    peer.reply(&t, Response::Err(ErrorType::Auth));
    assert_eq!(
        a.recv_timeout(WAIT).unwrap(),
        Err(SmpError::Server(ErrorType::Auth))
    );
}

#[test]
fn answer_for_another_queue_is_refused() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let a = sub(&rig, b"q");
    let t = peer.recv(WAIT).unwrap().remove(0);
    peer.send(&[(&t.corr_id, b"other", Response::Ok)]);
    assert!(matches!(
        a.recv_timeout(WAIT).unwrap(),
        Err(SmpError::Unexpected(_))
    ));
}

#[test]
fn push_sharing_a_block_with_a_reply_reaches_both() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let a = sub(&rig, b"q");
    let t = peer.recv(WAIT).unwrap().remove(0);
    let msg = Response::Msg {
        msg_id: vec![1; 24],
        body: vec![5, 6],
    };
    peer.send(&[(&[], b"q", msg), (&t.corr_id, b"q", Response::Sok(None))]);
    assert_eq!(a.recv_timeout(WAIT).unwrap(), Ok(Response::Sok(None)));
    assert_eq!(
        rig.next_event(),
        SmpEventKind::Msg {
            entity_id: b"q".to_vec(),
            msg_id: vec![1; 24],
            body: vec![5, 6]
        }
    );
}

#[test]
fn msg_that_answers_a_command_is_also_an_event() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let a = rig
        .conn
        .send(Request::new(
            Command::Ack {
                msg_id: vec![1; 24],
            },
            b"q".to_vec(),
            Some(private(Alg::Ed25519)),
        ))
        .unwrap();
    let t = peer.recv(WAIT).unwrap().remove(0);
    peer.reply(
        &t,
        Response::Msg {
            msg_id: vec![2; 24],
            body: vec![1],
        },
    );
    assert!(matches!(
        a.recv_timeout(WAIT).unwrap(),
        Ok(Response::Msg { .. })
    ));
    assert!(matches!(rig.next_event(), SmpEventKind::Msg { .. }));
}

#[test]
fn end_and_deld_pushes_are_events() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    peer.push(b"q", Response::End);
    peer.push(b"r", Response::Deld);
    assert_eq!(
        rig.next_event(),
        SmpEventKind::End {
            entity_id: b"q".to_vec()
        }
    );
    assert_eq!(
        rig.next_event(),
        SmpEventKind::Deld {
            entity_id: b"r".to_vec()
        }
    );
}

#[test]
fn writer_authorises_with_the_session_it_sends_on() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let ed = keypair(Alg::Ed25519);
    let x = keypair(Alg::X25519);
    rig.conn
        .send(Request::new(
            Command::Sub,
            b"q".to_vec(),
            Some(ed.private.clone()),
        ))
        .unwrap();
    rig.conn
        .send(Request::new(
            Command::Skey(x.public.clone()),
            b"s".to_vec(),
            Some(x.private.clone()),
        ))
        .unwrap();
    let got = peer.recv_all(Duration::from_millis(200));
    assert_eq!(got.len(), 2);
    for i in &got {
        let msg = authorized(&peer.session_id, &i.corr_id, &i.entity_id, &i.command).unwrap();
        if i.entity_id == b"q" {
            assert!(
                ed.public
                    .verify(&Signature::from_raw(Alg::Ed25519, &i.auth).unwrap(), &msg)
            );
        } else {
            let nonce = CbNonce::from_slice(&i.corr_id).unwrap();
            assert!(cb_verify(
                &x.public,
                &peer.dh.private,
                &nonce,
                &i.auth,
                &msg
            ));
        }
    }
}

#[test]
fn commands_wait_no_longer_than_the_command_timeout() {
    let cfg = ConnConfig {
        command_timeout: Duration::from_millis(60),
        max_timeouts: 100,
        ..calm()
    };
    let rig = rig(cfg, 23);
    let _peer = rig.up();
    let a = sub(&rig, b"q");
    assert_eq!(a.recv_timeout(WAIT).unwrap(), Err(SmpError::Timeout));
}

#[test]
fn too_many_waiting_commands_are_refused() {
    let cfg = ConnConfig {
        max_pending: 2,
        ..calm()
    };
    let rig = rig(cfg, 23);
    let peer = rig.up();
    let answers: Vec<_> = (0..3).map(|i| sub(&rig, &[i])).collect();
    assert_eq!(
        answers[2].recv_timeout(WAIT).unwrap(),
        Err(SmpError::TooManyPending)
    );
    assert_eq!(peer.recv_all(Duration::from_millis(100)).len(), 2);
}

#[test]
fn losing_the_link_fails_waiters_and_reconnects_with_a_new_up_event() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let a = sub(&rig, b"q");
    peer.recv(WAIT).unwrap();
    peer.drop_link();
    assert_eq!(a.recv_timeout(WAIT).unwrap(), Err(SmpError::NotConnected));
    assert!(matches!(rig.next_event(), SmpEventKind::Down(_)));
    let second = rig.up();
    assert!(rig.conn.is_up());
    let b = sub(&rig, b"q");
    let t = second.recv(WAIT).unwrap().remove(0);
    second.reply(&t, Response::Ok);
    assert_eq!(b.recv_timeout(WAIT).unwrap(), Ok(Response::Ok));
}

#[test]
fn commands_while_down_are_refused_at_once() {
    let cfg = ConnConfig {
        reconnect_min: Duration::from_millis(300),
        reconnect_max: Duration::from_millis(300),
        ..calm()
    };
    let rig = rig(cfg, 23);
    let peer = rig.up();
    peer.drop_link();
    while !matches!(rig.next_event(), SmpEventKind::Down(_)) {}
    assert_eq!(
        rig.conn
            .send(Request::new(Command::Sub, b"q".to_vec(), None))
            .err(),
        Some(SmpError::NotConnected)
    );
}

#[test]
fn failed_connect_attempts_are_reported_and_retried() {
    let (connector, peers) = FakeConnector::new(23);
    connector.refuse_next(2);
    let (tx, events) = bounded(64);
    let conn = SmpConnection::open_with(
        server(),
        calm(),
        connector as _,
        Arc::new(SystemClock::new()),
        tx,
    )
    .unwrap();
    let kinds: Vec<_> = std::iter::from_fn(|| events.recv_timeout(WAIT).ok())
        .take(3)
        .map(|e| e.kind)
        .collect();
    assert!(matches!(kinds[0], SmpEventKind::ConnectFailed(_)));
    assert!(matches!(kinds[1], SmpEventKind::ConnectFailed(_)));
    assert!(matches!(kinds[2], SmpEventKind::Up(_)));
    assert!(peers.recv_timeout(WAIT).is_ok());
    drop(conn);
}

#[test]
fn close_fails_waiters_stops_the_threads_and_refuses_more() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    let a = sub(&rig, b"q");
    peer.recv(WAIT).unwrap();
    rig.conn.close();
    assert!(a.recv_timeout(WAIT).unwrap().is_err());
    assert!(peer.is_closed());
    assert_eq!(
        rig.conn
            .send(Request::new(Command::Sub, b"q".to_vec(), None))
            .err(),
        Some(SmpError::Closed)
    );
    rig.conn.close();
}

#[test]
fn session_info_tells_the_negotiated_version() {
    let rig = rig(calm(), 21);
    let _peer = rig.up();
    let info = rig.conn.session().unwrap();
    assert_eq!((info.version, info.block_encrypted), (21, false));
}
