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
    /// The relays as configured, in order; `cfg.servers` is this with the disabled ones last.
    configured: Vec<SmpServer>,
}

impl NativeAgent {
    /// `relays` are the Hello's addresses; the `smp://` ones are used, or the presets when none is.
    #[must_use]
    pub fn new(relays: &[String], log: Log) -> (Self, Receiver<SmpEvent>) {
        let named = relays.iter().filter(|r| r.starts_with("smp://"));
        let any_named = named.clone().next().is_some();
        let mut servers: Vec<SmpServer> = named.filter_map(|r| r.parse().ok()).collect();
        // Relays the user named but that do not parse leave none: queues fail rather than
        // reach the presets the user did not choose.
        if servers.is_empty() && !any_named {
            servers = presets::defaults();
        }
        let configured = servers.clone();
        let mut cfg = AgentConfig::new(servers);
        cfg.presets = presets::all();
        let (tx, rx) = unbounded();
        (
            Self {
                client: AgentClient::new(cfg, tx),
                log,
                configured,
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

    fn create_address(
        &mut self,
        store: &mut Store,
        short: bool,
        user_data: Vec<u8>,
    ) -> Result<ConnId, AgentFailure> {
        let id = self
            .client
            .create_address(store, short, user_data)
            .map_err(failed)?;
        Ok(conn_id(id))
    }

    fn create_group_address(
        &mut self,
        store: &mut Store,
        user_data: Vec<u8>,
        client_data: String,
    ) -> Result<ConnId, AgentFailure> {
        let id = self
            .client
            .create_address_with(store, true, user_data, Some(client_data))
            .map_err(failed)?;
        Ok(conn_id(id))
    }

    fn accept_request(
        &mut self,
        store: &mut Store,
        invitation: &[u8],
        conn_info: Vec<u8>,
        pq: bool,
    ) -> Result<ConnId, AgentFailure> {
        let id = self
            .client
            .accept_contact(store, invitation, conn_info, pq)
            .map_err(failed)?;
        Ok(conn_id(id))
    }

    fn reject_request(&mut self, store: &mut Store, invitation: &[u8]) -> Result<(), AgentFailure> {
        self.client
            .reject_contact(store, invitation)
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

    fn ratchet_ad(&mut self, _: &mut Store, conn: &ConnId) -> Result<Vec<u8>, AgentFailure> {
        let c = self.client.conn(native_conn(conn)).map_err(failed)?;
        let ratchet = c.ratchet.as_ref();
        ratchet
            .map(|r| r.associated_data().to_vec())
            .ok_or_else(|| AgentFailure::Other("the connection has no ratchet yet".into()))
    }

    fn synchronize_ratchet(
        &mut self,
        store: &mut Store,
        conn: &ConnId,
    ) -> Result<(), AgentFailure> {
        self.client
            .synchronize_ratchet(store, native_conn(conn), false)
            .map_err(failed)
    }

    fn set_disabled_relays(&mut self, disabled: &[String]) {
        let down: Vec<SmpServer> = disabled.iter().filter_map(|r| r.parse().ok()).collect();
        let (off, on): (Vec<_>, Vec<_>) = self
            .configured
            .iter()
            .cloned()
            .partition(|s| down.contains(s));
        // Queues go on the first server: the working ones lead, and every server stays listed
        // for its password and so its connection is kept.
        self.client.cfg.servers = on.into_iter().chain(off).collect();
    }

    fn drain_events(&mut self) -> Vec<AgentEvent> {
        self.client
            .drain_events()
            .into_iter()
            .filter_map(|e| event(e, &self.log))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "smp://jZxOZYTgS4Q3Mp5r4wJW5ruxuQAPb8y5pa4a1DI2l6Y=@127.0.0.1:5223";
    const B: &str = "smp://jZxOZYTgS4Q3Mp5r4wJW5ruxuQAPb8y5pa4a1DI2l6Y=@127.0.0.2:5999";

    fn agent(relays: &[&str]) -> NativeAgent {
        let relays: Vec<String> = relays.iter().map(|r| (*r).to_owned()).collect();
        NativeAgent::new(&relays, Log::discard()).0
    }

    fn first(a: &NativeAgent) -> Option<String> {
        a.client.cfg.servers.first().map(|s| s.hosts[0].clone())
    }

    #[test]
    fn custom_relays_are_the_agents_servers_and_the_presets_only_when_none_is_named() {
        let a = agent(&[A, B]);
        assert_eq!(a.client.cfg.servers.len(), 2);
        assert_eq!(first(&a).as_deref(), Some("127.0.0.1"));
        assert_eq!(agent(&[]).client.cfg.servers, presets::defaults());
    }

    #[test]
    fn rejected_custom_relays_never_fall_back_to_the_presets() {
        let a = agent(&["smp://bad"]);
        assert!(a.client.cfg.servers.is_empty());
    }

    #[test]
    fn disabled_relays_come_last_and_enabling_restores_the_order() {
        let mut a = agent(&[A, B]);
        a.set_disabled_relays(&[A.to_owned()]);
        assert_eq!(first(&a).as_deref(), Some("127.0.0.2"));
        assert_eq!(
            a.client.cfg.servers.len(),
            2,
            "still listed for its password"
        );
        a.set_disabled_relays(&[]);
        assert_eq!(first(&a).as_deref(), Some("127.0.0.1"));
    }
}
