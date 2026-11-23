//! Delete a connection: DEL our queue, forget every record of it, and close servers nothing uses (Agent.hs `deleteConnection'`).
use std::collections::HashSet;

use crate::smp::SmpServer;
use crate::store::Store;

use super::queues::del_request;
use crate::agent::client::{AgentClient, ConnId, OpKind};
use crate::agent::error::{AgentError, Result};
use crate::agent::records::{self, Batch};

impl AgentClient {
    pub fn delete(&mut self, store: &mut Store, conn: ConnId) -> Result<()> {
        let c = self.conns.remove(&conn).ok_or(AgentError::ConnNotFound)?;
        if let Some(rq) = c.rec.rcv.as_ref().filter(|r| !r.rcv_id.is_empty()) {
            self.by_rcv.remove(&rq.rcv_id);
            // Best effort: a queue left behind expires on the server.
            self.request(conn, OpKind::Del, &rq.server.clone(), del_request(rq));
        }
        let mut b = Batch::default();
        for prefix in records::conn_prefixes(conn) {
            for k in store.keys_with_prefix(records::NS, &prefix)? {
                b.delete(k);
            }
        }
        b.apply(store)
    }

    /// Closes the connections to servers no queue, configured server or waiting command names.
    pub(crate) fn close_unused_servers(&mut self) {
        let mut used: HashSet<SmpServer> = self.cfg.servers.iter().cloned().collect();
        for c in self.conns.values() {
            used.extend(c.rec.rcv.iter().map(|q| q.server.clone()));
            used.extend(c.rec.snd.iter().map(|q| q.server.clone()));
            // A switch's new or old queue: its server must stay open between the switch's requests.
            used.extend(c.rec.switch.iter().map(|s| s.queue().server.clone()));
            used.extend(c.rec.request.iter().map(|r| r.queue.address.server.clone()));
            if let Ok((s, _)) = super::steps::short_target(&c.rec) {
                used.insert(s);
            }
        }
        used.extend(self.ops.servers());
        self.servers.retain(|s| used.contains(s));
    }
}
