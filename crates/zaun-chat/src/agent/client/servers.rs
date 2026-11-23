//! One SMP connection per server, opened when a queue first needs it and closed when no connection uses the server.
use std::collections::HashMap;
use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::smp::{
    Clock, ConnConfig, Connector, Request, SmpConnection, SmpError, SmpEvent, SmpServer,
};

pub(crate) struct Servers {
    /// Eviction: `retain` closes the servers no stored queue names.
    slots: HashMap<SmpServer, SmpConnection>,
    connector: Arc<dyn Connector>,
    clock: Arc<dyn Clock>,
    cfg: ConnConfig,
    events: Sender<SmpEvent>,
}

impl Servers {
    pub(crate) fn new(
        cfg: ConnConfig,
        connector: Arc<dyn Connector>,
        clock: Arc<dyn Clock>,
        events: Sender<SmpEvent>,
    ) -> Self {
        Self {
            slots: HashMap::new(),
            connector,
            clock,
            cfg,
            events,
        }
    }

    /// Opens the connection if there is none; it reports `Up` on the event channel.
    pub(crate) fn ensure(&mut self, server: &SmpServer) -> Result<(), SmpError> {
        if self.slots.contains_key(server) {
            return Ok(());
        }
        let conn = SmpConnection::open_with(
            server.clone(),
            self.cfg.clone(),
            Arc::clone(&self.connector),
            Arc::clone(&self.clock),
            self.events.clone(),
        )?;
        self.slots.insert(server.clone(), conn);
        Ok(())
    }

    pub(crate) fn send(
        &mut self,
        server: &SmpServer,
        request: Request,
        tag: u64,
    ) -> Result<(), SmpError> {
        self.ensure(server)?;
        match self.slots.get(server) {
            Some(c) => c.send_tagged(request, tag),
            None => Err(SmpError::Closed),
        }
    }

    /// Closes every server `keep` says no queue uses any more.
    pub(crate) fn retain(&mut self, keep: impl Fn(&SmpServer) -> bool) {
        let gone: Vec<SmpServer> = self.slots.keys().filter(|s| !keep(s)).cloned().collect();
        for srv in gone {
            if let Some(conn) = self.slots.remove(&srv) {
                // `close` joins the connection thread, which may be inside a TCP connect for up to
                // `connect_timeout`; the engine thread must not wait for that.
                let spawned = std::thread::Builder::new()
                    .name("smp-close".into())
                    .spawn(move || conn.close());
                drop(spawned); // without a thread the closure is dropped, which closes the connection here
            }
        }
    }

    /// Waits for every connection thread; for shutdown, not for the engine loop.
    pub(crate) fn close_all(&mut self) {
        for c in self.slots.values() {
            c.close();
        }
        self.slots.clear();
    }
}
