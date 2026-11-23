//! Requests arriving on an address, and the answer to each: accept joins the requester's invitation, reject forgets it (Agent.hs `smpInvitation`, `acceptContact'`, `rejectContact'`).
use crate::store::Store;

use crate::agent::client::{AgentClient, AgentEvent, ConnId};
use crate::agent::error::{AgentError, Result};
use crate::agent::links::{ConnectionLink, ConnectionRequestUri};
use crate::agent::records::{self, Batch, ConnStatus, StoredInvitation};

/// The request's id for the engine: the address's connection, then the request's number.
fn invitation_id(conn: ConnId, n: i64) -> Vec<u8> {
    [conn.0.to_be_bytes(), n.to_be_bytes()].concat()
}

fn parse_id(inv: &[u8]) -> Result<(ConnId, i64)> {
    let bad = || AgentError::Prohibited("unknown request");
    let (c, n) = inv.split_at_checked(8).ok_or_else(bad)?;
    let conn = i64::from_be_bytes(c.try_into().map_err(|_| bad())?);
    let n = i64::from_be_bytes(n.try_into().map_err(|_| bad())?);
    Ok((ConnId(conn), n))
}

impl AgentClient {
    /// A request on the address `id`: kept until answered, and told to the engine.
    pub(crate) fn on_contact_request(
        &mut self,
        store: &mut Store,
        id: ConnId,
        conn_req: &ConnectionRequestUri,
        conn_info: Vec<u8>,
    ) -> Result<()> {
        if self.conn(id)?.rec.status != ConnStatus::Address {
            return Err(AgentError::Prohibited(
                "request sent to a message connection",
            ));
        }
        let n = store.next_id("agent.inv")?;
        let stored = StoredInvitation {
            conn_req: conn_req.to_uri(),
            conn_info: conn_info.clone(),
        };
        let mut b = Batch::default();
        b.invitation(id, n, &stored)?;
        b.apply(store)?;
        self.emit(AgentEvent::Req {
            conn: id,
            inv_id: invitation_id(id, n),
            conn_info,
        });
        Ok(())
    }

    /// Joins the requester's invitation with `conn_info` (chat's `x.info`); the request is forgotten once that starts.
    pub fn accept_contact(
        &mut self,
        store: &mut Store,
        inv_id: &[u8],
        conn_info: Vec<u8>,
        pq: bool,
    ) -> Result<ConnId> {
        let (addr, n) = parse_id(inv_id)?;
        let stored = self.stored_invitation(store, addr, n)?;
        let req = ConnectionRequestUri::parse(&stored.conn_req)?;
        let conn = self.join(store, &ConnectionLink::Full(req), conn_info, pq)?;
        self.forget_invitation(store, addr, n)?;
        Ok(conn)
    }

    /// Forgets the request and sends nothing: the requester is not told (`rejectContact'` without a reason).
    pub fn reject_contact(&mut self, store: &mut Store, inv_id: &[u8]) -> Result<()> {
        let (addr, n) = parse_id(inv_id)?;
        self.stored_invitation(store, addr, n)?;
        self.forget_invitation(store, addr, n)
    }

    fn stored_invitation(
        &self,
        store: &mut Store,
        addr: ConnId,
        n: i64,
    ) -> Result<StoredInvitation> {
        records::load(store, &records::invitation_key(addr, n))?
            .ok_or(AgentError::Prohibited("unknown request"))
    }

    fn forget_invitation(&self, store: &mut Store, addr: ConnId, n: i64) -> Result<()> {
        let mut b = Batch::default();
        b.delete(records::invitation_key(addr, n));
        b.apply(store)
    }
}
