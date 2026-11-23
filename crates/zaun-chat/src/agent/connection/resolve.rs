//! The joiner's short link: LKEY secured the queue and brought the link data (Agent.hs `resolveShortLink`, Client.hs `secureGetQueueLink`).
use crate::smp::Response;
use crate::store::Store;

use crate::agent::client::{AgentClient, ConnId};
use crate::agent::error::{AgentError, Result};
use crate::agent::links::{BinaryConnReq, ConnShortLink, ConnectionMode, ConnectionRequestUri};
use crate::agent::records::Batch;
use crate::agent::short_link::{EncLinkData, decrypt_link_data, inv_short_link_kdf};

impl AgentClient {
    /// LKEY answered with the link data: it secured the queue, so the reply queue is next.
    pub(crate) fn on_link_resolved(
        &mut self,
        store: &mut Store,
        id: ConnId,
        r: Response,
    ) -> Result<()> {
        let Response::Lnk { sender_id, data } = r else {
            return Err(AgentError::Prohibited("LKEY answered without LNK"));
        };
        let (mut rec, _) = self.editable(id)?;
        let link = ConnShortLink::parse(rec.link.as_deref().unwrap_or_default())?;
        let key = inv_short_link_kdf(link.link_key())?;
        let enc = EncLinkData {
            fixed: data.fixed,
            user: data.user,
        };
        let (fixed, _) =
            decrypt_link_data(ConnectionMode::Invitation, link.link_key(), &key, &enc)?;
        let BinaryConnReq::Invitation { data, e2e } = fixed.link_conn_req else {
            return Err(AgentError::Link("not an invitation".into()));
        };
        if data.smp_queues.first().map(|q| &q.address.sender_id) != Some(&sender_id) {
            return Err(AgentError::Link("sender id differs from the link's".into()));
        }
        let req = ConnectionRequestUri::Invitation { data, e2e };
        let lkey = rec.lkey.take();
        let ratchet = self.prepare_join(&mut rec, &req, lkey)?;
        rec.link = None;
        self.save(store, rec, Some(ratchet), Batch::default())?;
        self.drive(store, id);
        Ok(())
    }
}
