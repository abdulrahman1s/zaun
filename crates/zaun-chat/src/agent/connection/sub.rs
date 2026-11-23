//! SUB of a connection's receive queue on each new session (Agent.hs `subscribeConnections`).
use crate::smp::{Response, SmpError};

use super::queues::sub_request;
use crate::agent::client::{AgentClient, ConnId, OpKind, is_temporary};
use crate::agent::error::{AgentError, Result};
use crate::agent::records::ConnStatus;

impl AgentClient {
    pub(super) fn drive_sub(&mut self, id: ConnId) {
        let Ok(c) = self.conn(id) else { return };
        let creating = matches!(
            c.rec.status,
            ConnStatus::Creating
                | ConnStatus::Replying
                | ConnStatus::Addressing
                | ConnStatus::Applying
        );
        let Some(rq) = c
            .rec
            .rcv
            .as_ref()
            .filter(|r| !r.rcv_id.is_empty() && !creating)
        else {
            return;
        };
        if c.sub.subscribed || c.sub.in_flight {
            return;
        }
        let (server, request) = (rq.server.clone(), sub_request(rq));
        let sent = self.request(id, OpKind::Sub, &server, request);
        if let Ok(c) = self.conn_mut(id) {
            c.sub.in_flight = sent;
        }
    }

    pub(crate) fn on_subscribed(&mut self, id: ConnId, answer: Result<Response, SmpError>) {
        let Ok(c) = self.conn_mut(id) else { return };
        c.sub.in_flight = false;
        match answer {
            Ok(_) => c.sub.subscribed = true,
            // A failed SUB is sent again on the next `Up`; a refused one is reported.
            Err(e) if is_temporary(&e) => {}
            Err(e) => {
                let err = AgentError::Smp(e);
                self.emit(crate::agent::client::AgentEvent::Err { conn: id, err });
            }
        }
    }
}
