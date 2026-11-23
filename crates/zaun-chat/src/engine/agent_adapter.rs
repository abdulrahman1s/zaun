//! `AgentPort` over the native agent client: ids, errors and events mapped, nothing else (ADR-0042).

use crossbeam_channel::{Receiver, unbounded};

use super::agent_map::{conn_id, event, failure, native_conn, native_msg};
use super::agent_port::{AgentEvent, AgentFailure, AgentMsgId, AgentPort, ConnId};
use super::log::Log;
use super::presets;
use crate::agent::AgentError;
use crate::agent::client::{AgentClient, AgentConfig};
use crate::agent::links::ConnectionLink;
use crate::smp::{SmpEvent, SmpServer};
use crate::store::Store;

pub struct NativeAgent {
    client: AgentClient,
    log: Log,
}

impl NativeAgent {
    /// `relays` are the Hello's addresses; the `smp://` ones are used, or the presets when none is.
    #[must_use]
    pub fn new(relays: &[String], log: Log) -> (Self, Receiver<SmpEvent>) {
        let mut servers: Vec<SmpServer> = relays
            .iter()
            .filter(|r| r.starts_with("smp://"))
            .filter_map(|r| r.parse().ok())
            .collect();
        if servers.is_empty() {
            servers = presets::defaults();
        }
        let mut cfg = AgentConfig::new(servers);
        cfg.presets = presets::all();
        let (tx, rx) = unbounded();
        (
            Self {
                client: AgentClient::new(cfg, tx),
                log,
            },
            rx,
        )
    }
}

fn failed(e: AgentError) -> AgentFailure {
    failure(&e)
}

impl AgentPort for NativeAgent {
    type Smp = SmpEvent;

    fn create_invitation(
        &mut self,
        store: &mut Store,
        pq: bool,
        short: bool,
        user_data: Vec<u8>,
    ) -> Result<ConnId, AgentFailure> {
        let id = self
            .client
            .create_invitation_with(store, pq, short, user_data)
            .map_err(failed)?;
        Ok(conn_id(id))
    }

    fn join(
        &mut self,
        store: &mut Store,
        link: &ConnectionLink,
        conn_info: Vec<u8>,
        pq: bool,
    ) -> Result<ConnId, AgentFailure> {
        let id = self
            .client
            .join(store, link, conn_info, pq)
            .map_err(failed)?;
        Ok(conn_id(id))
    }

    fn allow(
        &mut self,
        store: &mut Store,
        conn: &ConnId,
        conn_info: Vec<u8>,
    ) -> Result<(), AgentFailure> {
        self.client
            .allow(store, native_conn(conn), conn_info)
            .map_err(failed)
    }

    fn send(
        &mut self,
        store: &mut Store,
        conn: &ConnId,
        body: Vec<u8>,
    ) -> Result<AgentMsgId, AgentFailure> {
        let id = self
            .client
            .send(store, native_conn(conn), body)
            .map_err(failed)?;
        Ok(id.0)
    }

    fn ack(
        &mut self,
        store: &mut Store,
        conn: &ConnId,
        msg_id: AgentMsgId,
        receipt: Option<Vec<u8>>,
    ) -> Result<(), AgentFailure> {
        self.client
            .ack(store, native_conn(conn), native_msg(msg_id), receipt)
            .map_err(failed)
    }

    fn delete(&mut self, store: &mut Store, conn: &ConnId) -> Result<(), AgentFailure> {
        self.client.delete(store, native_conn(conn)).map_err(failed)
    }

    fn subscribe_all(&mut self, store: &mut Store) -> Result<(), AgentFailure> {
        self.client.subscribe_all(store).map_err(failed)
    }

    fn on_smp(&mut self, store: &mut Store, ev: SmpEvent) {
        self.client.on_smp(store, ev);
    }

    fn on_tick(&mut self, store: &mut Store, now_ms: i64) {
        self.client.on_tick(store, now_ms);
    }

    fn next_wake(&self) -> Option<i64> {
        self.client.next_wake()
    }

    fn close(&mut self) {
        self.client.close();
    }

    fn drain_events(&mut self) -> Vec<AgentEvent> {
        self.client
            .drain_events()
            .into_iter()
            .filter_map(|e| event(e, &self.log))
            .collect()
    }
}
