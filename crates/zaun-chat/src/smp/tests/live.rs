//! Against a real smp-server. Start a local smp-server and run
//! `ZAUN_TEST_SMP=smp://<fingerprint>@127.0.0.1:<port> cargo test -p zaun-chat --lib smp::tests::live -- --ignored`.
use std::time::Duration;

use crossbeam_channel::{Receiver, bounded};

use super::connection::WAIT;
use super::support::keypair;
use crate::crypto::Alg;
use crate::smp::{
    Command, ConnConfig, ErrorType, NewQueue, RcvMsg, Request, Response, SmpConnection, SmpError,
    SmpEvent, SmpEventKind, SmpServer, SubscriptionMode, decrypt_message, rcv_dh_secret,
};

fn open(server: &SmpServer) -> (SmpConnection, Receiver<SmpEvent>) {
    let (tx, rx) = bounded(64);
    let conn = SmpConnection::open(server.clone(), ConnConfig::default(), tx).unwrap();
    loop {
        match rx
            .recv_timeout(Duration::from_secs(20))
            .expect("server up")
            .kind
        {
            SmpEventKind::Up(info) => {
                assert!(info.version >= 15, "negotiated v{}", info.version);
                assert!(
                    info.block_encrypted,
                    "the Haskell client's block encryption must be on"
                );
                return (conn, rx);
            }
            SmpEventKind::ConnectFailed(e) => panic!("connect failed: {e}"),
            _ => {}
        }
    }
}

fn expect(r: Result<Response, SmpError>) -> Response {
    r.unwrap_or_else(|e| panic!("unexpected failure: {e}"))
}

#[test]
#[ignore = "needs a local smp-server; set ZAUN_TEST_SMP"]
fn queue_lifecycle_with_block_encryption_new_sub_skey_send_msg_ack_del() {
    let Ok(addr) = std::env::var("ZAUN_TEST_SMP") else {
        panic!("ZAUN_TEST_SMP is not set")
    };
    let server: SmpServer = addr.parse().unwrap();
    let (rcv, rcv_events) = open(&server);
    let (snd, _snd_events) = open(&server);

    let (rcv_auth, rcv_dh) = (keypair(Alg::Ed25519), keypair(Alg::X25519));
    let new = NewQueue::messaging(
        rcv_auth.public.clone(),
        rcv_dh.public.clone(),
        SubscriptionMode::Subscribe,
    );
    let Response::Ids(ids) = expect(rcv.call(Request::new(
        Command::New(Box::new(new)),
        vec![],
        Some(rcv_auth.private.clone()),
    ))) else {
        panic!("NEW must answer IDS")
    };
    let secret = rcv_dh_secret(&ids.rcv_dh_key, &rcv_dh.private).unwrap();

    assert_eq!(
        expect(rcv.call(Request::new(
            Command::Sub,
            ids.rcv_id.clone(),
            Some(rcv_auth.private.clone())
        ))),
        Response::Sok(None)
    );
    assert_eq!(
        expect(rcv.call(Request::new(Command::Ping, vec![], None))),
        Response::Pong
    );

    let snd_key = keypair(Alg::X25519);
    let skey = Command::Skey(snd_key.public.clone());
    assert_eq!(
        expect(snd.call(Request::new(
            skey,
            ids.snd_id.clone(),
            Some(snd_key.private.clone())
        ))),
        Response::Ok
    );

    let body = b"zaun live smp test".to_vec();
    let send = |auth: Option<crate::crypto::PrivateKey>| {
        snd.call(Request::new(
            Command::Send {
                notification: false,
                body: body.clone(),
            },
            ids.snd_id.clone(),
            auth,
        ))
    };
    assert_eq!(
        send(None),
        Err(SmpError::Server(ErrorType::Auth)),
        "an unauthorised SEND must be refused"
    );
    assert_eq!(
        send(Some(keypair(Alg::X25519).private)),
        Err(SmpError::Server(ErrorType::Auth))
    );
    assert_eq!(expect(send(Some(snd_key.private.clone()))), Response::Ok);

    let (entity_id, msg_id, enc) = loop {
        if let SmpEventKind::Msg {
            entity_id,
            msg_id,
            body,
        } = rcv_events.recv_timeout(WAIT).expect("MSG pushed").kind
        {
            break (entity_id, msg_id, body);
        }
    };
    assert_eq!(entity_id, ids.rcv_id);
    let RcvMsg::Message { body: plain, .. } = decrypt_message(&secret, &msg_id, &enc).unwrap()
    else {
        panic!("not a message")
    };
    assert_eq!(plain, body);

    assert_eq!(
        expect(rcv.call(Request::new(
            Command::Ack { msg_id },
            ids.rcv_id.clone(),
            Some(rcv_auth.private.clone())
        ))),
        Response::Ok
    );
    assert_eq!(
        expect(rcv.call(Request::new(
            Command::Del,
            ids.rcv_id.clone(),
            Some(rcv_auth.private.clone())
        ))),
        Response::Ok
    );
    assert_eq!(
        send(Some(snd_key.private)),
        Err(SmpError::Server(ErrorType::Auth)),
        "SEND after DEL"
    );

    snd.close();
    rcv.close();
}
