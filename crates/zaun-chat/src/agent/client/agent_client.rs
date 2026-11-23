//! The agent's state, owned by the engine thread, and the plumbing every flow uses: emit, request, save.
use std::collections::HashMap;
use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::smp::{Clock, Connector, Request, SmpEvent, SmpServer, SystemClock, TlsConnector};

use super::clock::{SystemWallClock, WallClock};
use super::config::{AgentConfig, ms};
use super::conn::Conn;
use super::event::AgentEvent;
use super::ids::ConnId;
use super::ops::{Op, OpKind, Ops};
use super::servers::Servers;
use crate::agent::error::{AgentError, Result};

pub struct AgentClient {
    pub(crate) cfg: AgentConfig,
    pub(crate) clock: Arc<dyn WallClock>,
    pub(crate) servers: Servers,
    pub(crate) ops: Ops,
    /// Every stored connection is loaded. Eviction: an entry leaves on `delete`.
    pub(crate) conns: HashMap<ConnId, Conn>,
    /// Receive queue id to its connection; follows `conns`.
    pub(crate) by_rcv: HashMap<Vec<u8>, ConnId>,
    /// Eviction: emptied by `drain_events`, which the engine calls after each call into the agent.
    pub(crate) events: Vec<AgentEvent>,
    /// Tests stop a queue rotation between its stored steps, to restart there.
    #[cfg(test)]
    pub(crate) hold_switch: bool,
}

impl AgentClient {
    /// Connects over TCP and TLS. SMP events arrive on the receiver of `smp_events`; pass each to `on_smp`.
    #[must_use]
    pub fn new(cfg: AgentConfig, smp_events: Sender<SmpEvent>) -> Self {
        Self::with_parts(
            cfg,
            smp_events,
            Arc::new(TlsConnector),
            Arc::new(SystemClock::new()),
            Arc::new(SystemWallClock),
        )
    }

    /// With a given transport and clocks, for tests.
    #[must_use]
    pub fn with_parts(
        cfg: AgentConfig,
        smp_events: Sender<SmpEvent>,
        connector: Arc<dyn Connector>,
        smp_clock: Arc<dyn Clock>,
        clock: Arc<dyn WallClock>,
    ) -> Self {
        let servers = Servers::new(cfg.smp.clone(), connector, smp_clock, smp_events);
        Self {
            cfg,
            clock,
            servers,
            ops: Ops::default(),
            conns: HashMap::new(),
            by_rcv: HashMap::new(),
            events: Vec::new(),
            #[cfg(test)]
            hold_switch: false,
        }
    }

    /// The events since the last call, in order.
    pub fn drain_events(&mut self) -> Vec<AgentEvent> {
        std::mem::take(&mut self.events)
    }

    /// The earliest wall-clock ms at which `on_tick` has work: a retry or a handshake step.
    #[must_use]
    pub fn next_wake(&self) -> Option<i64> {
        self.conns
            .values()
            .flat_map(|c| {
                let send = (!c.sends.is_empty() && !c.send.in_flight).then_some(c.send.next_at);
                let waiting = c.step.retry_at > 0 && c.step.retry_at < i64::MAX;
                let step = (!c.step.in_flight && waiting).then_some(c.step.retry_at);
                [send, step]
            })
            .flatten()
            .min()
    }

    /// Closes every SMP connection; the state stays in the store.
    pub fn close(&mut self) {
        self.servers.close_all();
        self.ops.forget_server(|_| false);
    }

    pub(crate) fn now(&self) -> i64 {
        self.clock.now_ms()
    }

    pub(crate) fn emit(&mut self, e: AgentEvent) {
        self.events.push(e);
    }

    pub(crate) fn conn(&self, id: ConnId) -> Result<&Conn> {
        self.conns.get(&id).ok_or(AgentError::ConnNotFound)
    }

    pub(crate) fn conn_mut(&mut self, id: ConnId) -> Result<&mut Conn> {
        self.conns.get_mut(&id).ok_or(AgentError::ConnNotFound)
    }

    pub(crate) fn step_retry_after(&self) -> i64 {
        self.now() + ms(self.cfg.step_retry)
    }

    /// Sends `request` tagged; false when it could not even be queued (the server is down or busy).
    pub(crate) fn request(
        &mut self,
        conn: ConnId,
        kind: OpKind,
        server: &SmpServer,
        request: Request,
    ) -> bool {
        let tag = self.ops.next_tag();
        if self.servers.send(server, request, tag).is_err() {
            return false;
        }
        let server = server.clone();
        self.ops.insert(tag, Op { conn, kind, server });
        true
    }
}
