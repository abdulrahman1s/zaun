//! CreateInvite: a one-time invitation, one at a time, its link reported when the relay made it.

use crate::agent::short_link::{ContactShortLinkData, encode_short_link_data};
use crate::engine::agent_port::{AgentPort, ConnId};
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::sent_profile;
use crate::engine::records::{ConnKind, ConnRecord, UserRecord, keys};

/// Invitations nobody answered are kept up to this many; a new one evicts the oldest.
pub const MAX_INVITATIONS: usize = 16;

/// Every invitation is made with post-quantum keys and a short link (as 7.1.4 does).
pub const PQ: bool = true;

/// The most a short link's user data may hold (`FixedLinkData`'s sibling field, 13784 bytes).
const MAX_LINK_DATA: usize = 13784;

/// `{"profile":…,"business":false}`; a picture that would not fit is left out, the name stays.
fn link_data(user: &UserRecord) -> Result<Vec<u8>> {
    let mut profile = sent_profile(user);
    let mut data = encode_short_link_data(&ContactShortLinkData::new(profile.clone()));
    if data.as_ref().map_or(true, |d| d.0.len() > MAX_LINK_DATA) && profile.image.take().is_some() {
        data = encode_short_link_data(&ContactShortLinkData::new(profile));
    }
    data.map(|d| d.0)
        .map_err(|e| fail(Code::RequestFailed, e.to_string()))
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn create_invite(&mut self) -> Result<()> {
        if self.state.inviting.is_some() {
            return Ok(());
        }
        self.evict_invitations()?;
        let data = link_data(&self.state.user)?;
        let conn = self
            .agent
            .create_invitation(&mut self.store, PQ, true, data)?;
        let created = self.clock.wall_ms();
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Invitation {
                full: None,
                short: None,
                created,
            },
        })?;
        self.state.inviting = Some(conn);
        Ok(())
    }

    fn evict_invitations(&mut self) -> Result<()> {
        let mut open: Vec<(i64, ConnId)> = self
            .state
            .conns
            .values()
            .filter_map(|c| match c.kind {
                ConnKind::Invitation { created, .. } => Some((created, c.conn)),
                ConnKind::Friend { .. } => None,
            })
            .collect();
        if open.len() < MAX_INVITATIONS {
            return Ok(());
        }
        open.sort();
        let excess = open.len() + 1 - MAX_INVITATIONS;
        for (_, conn) in open.into_iter().take(excess) {
            self.forget_conn(&conn)?;
        }
        Ok(())
    }

    /// Deletes the connection at the agent and its record here.
    pub(crate) fn forget_conn(&mut self, conn: &ConnId) -> Result<()> {
        if let Err(e) = self.agent.delete(&mut self.store, conn) {
            self.log
                .line(format_args!("connection not deleted: {}", e.detail()));
        }
        self.store
            .delete(crate::engine::records::NS, &keys::conn(conn))?;
        self.state.conns.remove(conn);
        Ok(())
    }
}
