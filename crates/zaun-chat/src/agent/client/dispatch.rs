//! The engine's entry points for the network and timers: SMP events, tagged replies, ticks and the start (Agent.hs `processSMPTransmissions`, `subscribeConnections`).
use std::collections::VecDeque;

use crate::smp::{Response, SmpError, SmpEvent, SmpEventKind};
use crate::store::Store;

use super::agent_client::AgentClient;
use super::conn::Conn;
use super::event::AgentEvent;
use super::ids::ConnId;
use super::ops::OpKind;
use crate::agent::delivery::RetryMode;
use crate::agent::error::{AgentError, Result};
use crate::agent::records::{self, ConnRecord};

impl AgentClient {
    /// Loads every connection, then subscribes their queues and resumes unfinished steps and sends as servers come up.
    pub fn subscribe_all(&mut self, store: &mut Store) -> Result<()> {
        for id in records::conn_ids(store)? {
            let Some(rec) = records::load::<ConnRecord>(store, &records::conn_key(id))? else {
                continue;
            };
            let ratchet = records::load(store, &records::ratchet_key(id))?;
            let mut c = Conn::new(rec, ratchet);
            c.sends = VecDeque::from(records::pending_ids(store, id)?);
            if let Some(rcv) = c.rec.rcv.as_ref().filter(|r| !r.rcv_id.is_empty()) {
                self.by_rcv.insert(rcv.rcv_id.clone(), id);
            }
            self.conns.insert(id, c);
        }
        let ids: Vec<ConnId> = self.conns.keys().copied().collect();
        for id in ids {
            self.drive(store, id);
        }
        Ok(())
    }

    pub fn on_smp(&mut self, store: &mut Store, ev: SmpEvent) {
        let server = ev.server;
        match ev.kind {
            SmpEventKind::Up(_) => {
                self.emit(AgentEvent::Up {
                    server: std::sync::Arc::clone(&server),
                });
                self.on_server_up(store);
            }
            SmpEventKind::Down(_) => {
                for c in self.conns.values_mut() {
                    if c.rec.rcv.as_ref().is_some_and(|r| r.server == *server) {
                        c.sub.subscribed = false;
                    }
                }
                self.emit(AgentEvent::Down { server });
            }
            SmpEventKind::Msg {
                entity_id,
                msg_id,
                body,
            } => {
                if let Some(&id) = self.by_rcv.get(&entity_id) {
                    self.on_queue_msg(store, id, &msg_id, &body);
                }
            }
            SmpEventKind::End { entity_id } => {
                if let Some(&conn) = self.by_rcv.get(&entity_id) {
                    if let Some(c) = self.conns.get_mut(&conn) {
                        c.sub.subscribed = false;
                    }
                    self.emit(AgentEvent::End { conn });
                }
            }
            SmpEventKind::Deld { entity_id } => {
                if let Some(&conn) = self.by_rcv.get(&entity_id) {
                    let err = AgentError::Prohibited("queue deleted by the server");
                    self.emit(AgentEvent::Err { conn, err });
                }
            }
            SmpEventKind::Reply { tag, answer } => self.on_reply(store, tag, answer),
            SmpEventKind::ConnectFailed(_) | SmpEventKind::Unexpected { .. } => {}
        }
    }

    /// Retries due handshake steps and sends.
    pub fn on_tick(&mut self, store: &mut Store, now: i64) {
        let due: Vec<ConnId> = self
            .conns
            .iter()
            .filter(|(_, c)| {
                let step = !c.step.in_flight && c.step.retry_at > 0 && c.step.retry_at <= now;
                let send = !c.sends.is_empty() && !c.send.in_flight && c.send.next_at <= now;
                step || send
            })
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.drive(store, id);
        }
        self.close_unused_servers();
    }

    /// A new session: nothing is subscribed on it yet, and steps and sends waiting for the network go now.
    fn on_server_up(&mut self, store: &mut Store) {
        for c in self.conns.values_mut() {
            c.step.retry_at = 0;
            if c.send.retry.is_some_and(|r| r.mode == RetryMode::Fast) {
                c.send.next_at = 0;
            }
        }
        let ids: Vec<ConnId> = self.conns.keys().copied().collect();
        for id in ids {
            self.drive(store, id);
        }
    }

    fn on_reply(&mut self, store: &mut Store, tag: u64, answer: Result<Response, SmpError>) {
        let Some(op) = self.ops.take(tag) else {
            return;
        };
        let id = op.conn;
        if !self.conns.contains_key(&id) {
            return;
        }
        let r = match op.kind {
            OpKind::NewQueue | OpKind::Lkey | OpKind::Lget | OpKind::Skey | OpKind::Request => {
                self.on_step_answer(store, id, op.kind, answer)
            }
            OpKind::Sub => {
                self.on_subscribed(id, answer);
                Ok(())
            }
            OpKind::Send(msg) => self.on_send_answer(store, id, msg, answer),
            OpKind::Switch(tag) => self.on_switch_answer(store, id, tag, answer),
            // A lost ACK is answered by the server redelivering the message, which is then acked again.
            OpKind::Ack | OpKind::Del => Ok(()),
        };
        if let Err(err) = r {
            self.emit(AgentEvent::Err { conn: id, err });
        }
    }
}
