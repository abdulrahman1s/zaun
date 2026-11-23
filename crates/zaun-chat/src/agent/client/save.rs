//! Changing a connection: copy its record and ratchet, change the copies, then store and keep them together.
use crate::ratchet::Ratchet;
use crate::store::Store;

use super::agent_client::AgentClient;
use super::conn::Conn;
use super::ids::ConnId;
use crate::agent::error::Result;
use crate::agent::records::{Batch, ConnRecord};

impl AgentClient {
    /// Stores `rec`, its ratchet when given, and `batch` in one transaction, then keeps them in memory.
    pub(crate) fn save(
        &mut self,
        store: &mut Store,
        rec: ConnRecord,
        ratchet: Option<Ratchet>,
        mut batch: Batch,
    ) -> Result<()> {
        batch.conn(&rec)?;
        if let Some(r) = &ratchet {
            batch.ratchet(rec.id, r)?;
        }
        batch.apply(store)?;
        let c = self
            .conns
            .entry(rec.id)
            .or_insert_with(|| Conn::new(rec.clone(), None));
        if let Some(r) = ratchet {
            c.ratchet = Some(r);
        }
        if let Some(rcv) = &rec.rcv
            && !rcv.rcv_id.is_empty()
        {
            self.by_rcv.insert(rcv.rcv_id.clone(), rec.id);
        }
        c.rec = rec;
        Ok(())
    }

    /// The connection's record and ratchet to change; nothing changes in memory until `save`.
    pub(crate) fn editable(&self, id: ConnId) -> Result<(ConnRecord, Option<Ratchet>)> {
        let c = self.conn(id)?;
        Ok((c.rec.clone(), c.ratchet.clone()))
    }
}
