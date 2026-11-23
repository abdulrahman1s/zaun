//! Join a one-time invitation: resolve a short link with LKEY, secure the inviter's queue, create the reply queue, queue the confirmation (Agent.hs `joinConn`, `startJoinInvitation`, `secureConfirmQueue`; HANDSHAKE.md §2).
use crate::crypto::{Alg, KeyPair, PrivateKey};
use crate::encoding::Encode;
use crate::ratchet::{Ratchet, RatchetVersions, compatible_e2e_version};
use crate::store::Store;

use super::params::rcv_params;
use super::queues::{new_rcv_queue, new_snd_queue};
use crate::agent::SmpQueueInfo;
use crate::agent::client::{AgentClient, ConnId};
use crate::agent::error::{AgentError, Result};
use crate::agent::links::{
    ConnShortLink, ConnectionLink, ConnectionRequestUri, restore_short_link,
};
use crate::agent::records::{Batch, ConnRecord, ConnStatus};
use crate::agent::version::{CURRENT_AGENT, supported_agent_range, supported_client_range};

impl AgentClient {
    /// `conn_info` is chat's `x.info` with our profile. The inviter's `Info` and then `Con` follow.
    pub fn join(
        &mut self,
        store: &mut Store,
        link: &ConnectionLink,
        conn_info: Vec<u8>,
        pq: bool,
    ) -> Result<ConnId> {
        let id = ConnId(store.next_id("agent.conn")?);
        let mut rec = ConnRecord::new(id, ConnStatus::Resolving, CURRENT_AGENT, pq);
        rec.conn_info = Some(conn_info);
        let ratchet = match link {
            ConnectionLink::Full(req) => Some(self.prepare_join(&mut rec, req, None)?),
            ConnectionLink::Short(l @ ConnShortLink::Invitation { .. }) => {
                let l = restore_short_link(&self.cfg.presets, l.clone());
                rec.link = Some(l.to_uri());
                // X25519 like 7.1.4's `sndAuthAlg`: this key also secures the queue and sends to it.
                rec.lkey = Some(KeyPair::generate(Alg::X25519)?.private);
                None
            }
            // TODO(phase 3): contact addresses, `AgentInvitation` to the address queue.
            ConnectionLink::Short(ConnShortLink::Contact { .. }) => {
                return Err(AgentError::Unsupported("contact addresses"));
            }
        };
        self.save(store, rec, ratchet, Batch::default())?;
        self.drive(store, id);
        Ok(id)
    }

    /// `startJoinInvitation`: versions, the send queue and the joining ratchet. Secured already when LKEY ran.
    pub(super) fn prepare_join(
        &self,
        rec: &mut ConnRecord,
        req: &ConnectionRequestUri,
        lkey: Option<PrivateKey>,
    ) -> Result<Ratchet> {
        let ConnectionRequestUri::Invitation { data, e2e } = req else {
            // TODO(phase 3): joining a contact address.
            return Err(AgentError::Unsupported("contact addresses"));
        };
        let agent_v = data
            .agent_v_range
            .highest_common(&supported_agent_range())
            .ok_or(AgentError::Version)?;
        let q = data
            .smp_queues
            .first()
            .ok_or(AgentError::Parse("link without queues"))?;
        let client_v = q
            .client_v_range
            .highest_common(&supported_client_range())
            .ok_or(AgentError::Version)?;
        let e2e_v = compatible_e2e_version(&e2e.v_range).ok_or(AgentError::Version)?;
        let rcv = rcv_params(e2e, e2e_v)?;
        let (ratchet, reply) = Ratchet::join(
            RatchetVersions::new(e2e_v),
            &rcv,
            crate::ratchet::PqSupport(rec.pq),
        )?;
        let sq = new_snd_queue(&SmpQueueInfo::from_uri(q, client_v), client_v, lkey)?;
        rec.agent_version = agent_v;
        rec.e2e_reply = Some(reply.to_bytes()?);
        rec.status = if sq.secured {
            rec.rcv = Some(new_rcv_queue(self.reply_server()?)?);
            ConnStatus::Replying
        } else {
            ConnStatus::Securing
        };
        rec.snd = Some(sq);
        Ok(ratchet)
    }

    pub(super) fn reply_server(&self) -> Result<crate::smp::SmpServer> {
        let s = self.cfg.servers.first();
        s.cloned()
            .ok_or(AgentError::Prohibited("no SMP server configured"))
    }
}
