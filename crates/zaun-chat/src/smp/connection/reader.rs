//! The reader thread of one session: matches answers to waiting commands by corrId and forwards pushes (Client.hs `receiveSMP`).
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::smp::block::RcvBlocks;
use crate::smp::error::SmpError;
use crate::smp::events::SmpEventKind;
use crate::smp::responses::Response;
use crate::smp::transmission::{Incoming, parse_payload};
use crate::smp::transport::Reader;

use super::shared::{Shared, lock};

/// Reads blocks until the connection fails; returns why.
pub fn run(
    sh: &Arc<Shared>,
    mut reader: Box<dyn Reader>,
    mut rcv: RcvBlocks,
    version: u16,
) -> SmpError {
    loop {
        let payload = match rcv.read(&mut *reader) {
            Ok(p) => p,
            Err(e) => return e,
        };
        sh.last_rx.store(sh.clock.now_ms(), Ordering::Release);
        let transmissions = match parse_payload(version, &payload) {
            Ok(t) => t,
            Err(e) => return e.into(),
        };
        for t in transmissions {
            if !dispatch(sh, version, t) {
                return SmpError::Closed;
            }
        }
    }
}

/// False when the owner stopped listening.
fn dispatch(sh: &Shared, version: u16, t: Incoming) -> bool {
    let decoded = Response::decode(version, &t.command);
    let waiting = if t.corr_id.is_empty() {
        None
    } else {
        lock(&sh.pending).take(&t.corr_id)
    };
    let Some(entry) = waiting else {
        return match decoded {
            Ok(r) => push(sh, t.entity_id, r),
            Err(_) => true, // an unreadable push has no one to be told to
        };
    };
    // A MSG that answers SUB, ACK or GET is also an event, so the owner has one path for messages.
    let kept = match &decoded {
        Ok(r @ Response::Msg { .. }) => push(sh, t.entity_id.clone(), r.clone()),
        _ => true,
    };
    let result = match decoded {
        Ok(Response::Err(e)) => Err(SmpError::Server(e)),
        Ok(_) if entry.entity_id != t.entity_id => {
            Err(SmpError::Unexpected("response for another queue"))
        }
        Ok(r) => Ok(r),
        Err(e) => Err(e.into()),
    };
    sh.answer(&entry.reply, result);
    kept
}

fn push(sh: &Shared, entity_id: Vec<u8>, r: Response) -> bool {
    sh.emit(match r {
        Response::Msg { msg_id, body } => SmpEventKind::Msg {
            entity_id,
            msg_id,
            body,
        },
        Response::End => SmpEventKind::End { entity_id },
        Response::Deld => SmpEventKind::Deld { entity_id },
        other @ (Response::Ids(_)
        | Response::Lnk { .. }
        | Response::Ok
        | Response::Sok(_)
        | Response::Nid { .. }
        | Response::Info(_)
        | Response::Err(_)
        | Response::Pong) => SmpEventKind::Unexpected {
            entity_id,
            response: other,
        },
    })
}
