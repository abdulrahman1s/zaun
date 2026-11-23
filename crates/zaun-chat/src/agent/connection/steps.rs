//! The handshake step each status waits on, sent from the status alone so that `Up`, a tick or a restart resumes it (Agent.hs `newConnSrv`, `joinConnSrv`, `allowConnection'`).
use crate::smp::{Request, SmpServer};
use crate::store::Store;

use super::queues::{new_request, skey_request};
use crate::agent::client::{AgentClient, ConnId, OpKind};
use crate::agent::error::{AgentError, Result};
use crate::agent::links::{ConnShortLink, ConnectionLink};
use crate::agent::records::{ConnRecord, ConnStatus};

impl AgentClient {
    /// Sends what the connection waits for: its handshake step, its SUB, the head of its sends.
    pub(crate) fn drive(&mut self, store: &mut Store, id: ConnId) {
        if let Err(err) = self.drive_step(id) {
            self.emit(crate::agent::client::AgentEvent::Err { conn: id, err });
        }
        self.drive_sub(id);
        self.kick(store, id);
    }

    pub(super) fn drive_step(&mut self, id: ConnId) -> Result<()> {
        let now = self.now();
        let c = self.conn(id)?;
        if c.step.in_flight || c.step.retry_at > now {
            return Ok(());
        }
        let Some((kind, server, request)) = self.step_request(&c.rec)? else {
            return Ok(());
        };
        let sent = self.request(id, kind, &server, request);
        let retry_at = self.step_retry_after();
        let c = self.conn_mut(id)?;
        c.step.in_flight = sent;
        c.step.retry_at = if sent { 0 } else { retry_at };
        Ok(())
    }

    pub(super) fn step_request(
        &self,
        rec: &ConnRecord,
    ) -> Result<Option<(OpKind, SmpServer, Request)>> {
        let missing = || AgentError::Prohibited("connection record without its queue");
        Ok(Some(match rec.status {
            ConnStatus::Creating | ConnStatus::Replying => {
                let rq = rec.rcv.as_ref().ok_or_else(missing)?;
                let link = rec
                    .new_link
                    .as_ref()
                    .filter(|_| rec.status == ConnStatus::Creating);
                let auth = self.basic_auth(&rq.server);
                (
                    OpKind::NewQueue,
                    rq.server.clone(),
                    new_request(rq, link, auth)?,
                )
            }
            ConnStatus::Securing | ConnStatus::Allowing => {
                let sq = rec.snd.as_ref().ok_or_else(missing)?;
                (OpKind::Skey, sq.server.clone(), skey_request(sq)?)
            }
            ConnStatus::Resolving => {
                let (server, link_id) = short_invitation(rec)?;
                let key = rec.lkey.as_ref().ok_or_else(missing)?;
                let cmd = crate::smp::Command::Lkey(key.public()?);
                (
                    OpKind::Lkey,
                    server,
                    Request::new(cmd, link_id, Some(key.clone())),
                )
            }
            ConnStatus::Invited
            | ConnStatus::Confirmed
            | ConnStatus::Accepting
            | ConnStatus::Joined
            | ConnStatus::Active => return Ok(None),
        }))
    }

    /// The configured password of `server`: records keep only the address.
    fn basic_auth(&self, server: &SmpServer) -> Option<String> {
        let s = self.cfg.servers.iter().find(|s| *s == server)?;
        s.basic_auth.clone()
    }
}

/// The server and link id of a joiner's unresolved short link.
pub(crate) fn short_invitation(rec: &ConnRecord) -> Result<(SmpServer, Vec<u8>)> {
    let link = rec
        .link
        .as_deref()
        .ok_or(AgentError::Prohibited("no link to resolve"))?;
    match ConnectionLink::parse(link)? {
        ConnectionLink::Short(ConnShortLink::Invitation {
            server, link_id, ..
        }) => Ok((server, link_id)),
        ConnectionLink::Short(ConnShortLink::Contact { .. }) | ConnectionLink::Full(_) => {
            Err(AgentError::Prohibited("not an invitation short link"))
        }
    }
}
