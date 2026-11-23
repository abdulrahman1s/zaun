//! Sending a connection's queued messages one at a time, in order, with retries (Agent.hs `runSmpQueueMsgDelivery`; fact sheet §13).
use crate::smp::{Command, ErrorType, Request, Response, SmpError};
use crate::store::Store;

use super::retry::RetryMode;
use crate::agent::client::{AgentClient, AgentEvent, AgentMsgId, ConnId, OpKind, is_temporary};
use crate::agent::error::{AgentError, Result};
use crate::agent::records::{self, PendingSend, SendKind};

impl AgentClient {
    /// Sends the head of the connection's queue unless one is in flight or waiting for its retry.
    pub(crate) fn kick(&mut self, store: &mut Store, id: ConnId) {
        let now = self.now();
        let Some(c) = self.conns.get(&id) else { return };
        let Some(&head) = c.sends.front() else { return };
        let Some(sq) = c
            .rec
            .snd
            .clone()
            .filter(|_| !c.send.in_flight && c.send.next_at <= now)
        else {
            return;
        };
        let p = match records::load::<PendingSend>(store, &records::send_key(id, head)) {
            Ok(Some(p)) => p,
            Ok(None) => {
                self.pop_send(id);
                return self.kick(store, id);
            }
            Err(err) => return self.emit(AgentEvent::Err { conn: id, err }),
        };
        if self.expired(id, &p, now) {
            if let Err(err) = self.drop_send(store, id, &p, AgentError::Expired) {
                self.emit(AgentEvent::Err { conn: id, err });
            }
            return self.kick(store, id);
        }
        let cmd = Command::Send {
            notification: p.kind.notifies(),
            body: p.body,
        };
        let request = Request::new(cmd, sq.snd_id.clone(), Some(sq.auth.clone()));
        if self.request(id, OpKind::Send(head), &sq.server, request) {
            if let Some(c) = self.conns.get_mut(&id) {
                c.send.in_flight = true;
            }
        } else {
            self.schedule_retry(id, RetryMode::Fast);
        }
    }

    pub(crate) fn on_send_answer(
        &mut self,
        store: &mut Store,
        id: ConnId,
        msg: AgentMsgId,
        answer: Result<Response, SmpError>,
    ) -> Result<()> {
        self.conn_mut(id)?.send.in_flight = false;
        let Some(p) = records::load::<PendingSend>(store, &records::send_key(id, msg))? else {
            return Ok(());
        };
        match answer {
            Ok(_) => self.on_sent(store, id, &p)?,
            Err(SmpError::Server(ErrorType::Quota)) if p.kind == SendKind::Confirmation => {
                self.drop_send(store, id, &p, AgentError::NotAvailable)?;
            }
            Err(e @ SmpError::Server(ErrorType::Quota)) => {
                if p.kind == SendKind::Msg {
                    let err = e.into();
                    self.emit(AgentEvent::Mwarn {
                        conn: id,
                        msg_id: p.id,
                        err,
                    });
                }
                self.schedule_retry(id, RetryMode::Slow);
            }
            Err(e) if is_temporary(&e) => self.schedule_retry(id, RetryMode::Fast),
            // AUTH and the other refusals: the queue will not take this message.
            Err(e) => self.drop_send(store, id, &p, e.into())?,
        }
        self.kick(store, id);
        Ok(())
    }
}
