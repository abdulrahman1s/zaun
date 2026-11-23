//! CreateAddress: one at a time, its link reported when the relay made it (Commands.hs `APICreateMyAddress`).

use zaun_chat_wire::{Event, RequestKind};

use crate::engine::agent_port::{AgentEvent, AgentFailure, AgentPort, ConnId};
use crate::engine::error::{Code, EngineError, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{AddressRecord, keys};
use crate::engine::relays::Retry;

use super::info::link_data;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn create_address(&mut self) -> Result<()> {
        if self.state.address.creating.is_some() {
            return Ok(());
        }
        // An address the engine already has is read back, not made again (`SEDuplicateContactLink`).
        if let Some(a) = &self.state.address.record {
            let shared = a.shared();
            self.emit(Event::Address(Some(shared)));
            return Ok(());
        }
        let data = link_data(&self.state.user)?;
        let conn = self.agent.create_address(&mut self.store, true, data)?;
        self.state.address.creating = Some(conn);
        Ok(())
    }

    pub(super) fn save_address(&mut self, a: AddressRecord) -> Result<()> {
        self.put(keys::ADDRESS, &a)?;
        self.state.address.record = Some(a);
        Ok(())
    }

    pub(crate) fn on_address_event(&mut self, event: AgentEvent) -> Result<()> {
        match event {
            AgentEvent::Address { conn, full, short } => self.on_address_made(conn, full, short),
            AgentEvent::Req {
                conn,
                invitation,
                conn_info,
                pq: _,
            } => self.on_contact_request(conn, invitation, &conn_info),
            AgentEvent::Invitation { .. }
            | AgentEvent::Conf { .. }
            | AgentEvent::Info { .. }
            | AgentEvent::Con { .. }
            | AgentEvent::Msg { .. }
            | AgentEvent::Sent { .. }
            | AgentEvent::Merr { .. }
            | AgentEvent::Rcvd { .. }
            | AgentEvent::Up { .. }
            | AgentEvent::Down { .. }
            | AgentEvent::Err { .. }
            | AgentEvent::RatchetSync { .. } => Ok(()),
        }
    }

    /// The relay made the queue. One that comes after a restart, for an address asked for before it, is adopted.
    fn on_address_made(&mut self, conn: ConnId, full: String, short: Option<String>) -> Result<()> {
        let asked = self.state.address.creating == Some(conn);
        if !asked && self.state.address.record.is_some() {
            self.log
                .line(format_args!("an address nobody asked for was dropped"));
            return self.drop_conn(&conn);
        }
        self.state.address.creating = None;
        let rec = AddressRecord {
            conn,
            full,
            short,
            accepted: Vec::new(),
        };
        let shared = rec.shared();
        self.save_address(rec)?;
        self.emit(Event::Address(Some(shared)));
        Ok(())
    }

    /// An agent failure on the address being made ends the attempt; one on a made address is only logged.
    pub(crate) fn on_address_error(
        &mut self,
        conn: &ConnId,
        err: &AgentFailure,
    ) -> Option<Result<()>> {
        if self.state.address.creating != Some(*conn) {
            return None;
        }
        if self.retry_after_check(Retry::Address, conn, err) {
            return Some(Ok(()));
        }
        self.state.address.creating = None;
        let e: EngineError = err.clone().into();
        let e = match err {
            AgentFailure::Network(_) => e,
            AgentFailure::BadLink(_) | AgentFailure::Other(_) => {
                fail(Code::RequestFailed, e.detail)
            }
        };
        let gone = self.drop_conn(conn);
        Some(gone.and_then(|()| self.fail_event(RequestKind::Address, e)))
    }

    /// Deletes a connection at the agent; one it no longer has is only logged.
    pub(super) fn drop_conn(&mut self, conn: &ConnId) -> Result<()> {
        if let Err(e) = self.agent.delete(&mut self.store, conn) {
            self.log
                .line(format_args!("address not deleted: {}", e.detail()));
        }
        Ok(())
    }
}
