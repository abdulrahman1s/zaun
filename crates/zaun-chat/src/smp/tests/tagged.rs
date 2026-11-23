//! `send_tagged`: answers arrive on the event channel under the caller's tag.
use super::connection::{WAIT, calm, rig};
use super::support::private;
use crate::crypto::Alg;
use crate::smp::{Command, ErrorType, Request, Response, SmpError, SmpEventKind};

fn sub(entity: &[u8]) -> Request {
    Request::new(Command::Sub, entity.to_vec(), Some(private(Alg::Ed25519)))
}

#[test]
fn tagged_answers_arrive_as_reply_events_with_their_tag() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    rig.conn.send_tagged(sub(b"q1"), 7).unwrap();
    rig.conn.send_tagged(sub(b"q2"), 8).unwrap();
    let got = peer.recv_all(std::time::Duration::from_millis(200));
    for t in got.iter().rev() {
        let r = if t.entity_id == b"q1" {
            Response::Ok
        } else {
            Response::Err(ErrorType::Auth)
        };
        peer.reply(t, r);
    }
    let mut answers = vec![rig.next_event(), rig.next_event()];
    answers.sort_by_key(|k| match k {
        SmpEventKind::Reply { tag, .. } => *tag,
        _ => 0,
    });
    assert_eq!(
        answers,
        [
            SmpEventKind::Reply {
                tag: 7,
                answer: Ok(Response::Ok)
            },
            SmpEventKind::Reply {
                tag: 8,
                answer: Err(SmpError::Server(ErrorType::Auth))
            },
        ]
    );
}

#[test]
fn a_tagged_command_cut_off_by_the_link_is_answered_with_an_error() {
    let rig = rig(calm(), 23);
    let peer = rig.up();
    rig.conn.send_tagged(sub(b"q"), 3).unwrap();
    peer.recv(WAIT).unwrap();
    peer.drop_link();
    let reply = std::iter::from_fn(|| Some(rig.next_event()))
        .find(|k| matches!(k, SmpEventKind::Reply { .. }))
        .unwrap();
    assert_eq!(
        reply,
        SmpEventKind::Reply {
            tag: 3,
            answer: Err(SmpError::NotConnected)
        }
    );
}
