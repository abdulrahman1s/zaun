//! The address and its requests back from the store at start, and the address told once the list is out.

use zaun_chat_wire::Event;

use super::AddressState;
use crate::engine::agent_port::AgentPort;
use crate::engine::machine::Engine;
use crate::engine::records::{AddressRecord, NS, RequestRecord, decode, keys};
use crate::store::StoreError;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn load_address(&mut self) -> Result<(), StoreError> {
        let mut state = AddressState::default();
        if let Some(b) = self.store.get(NS, keys::ADDRESS)? {
            state.record = Some(decode::<AddressRecord>(&b)?);
        }
        for key in self.store.keys_with_prefix(NS, keys::REQUEST_PREFIX)? {
            if let Some(b) = self.store.get(NS, &key)? {
                let r: RequestRecord = decode(&b)?;
                state.requests.insert(r.id, r);
            }
        }
        self.state.address = state;
        Ok(())
    }

    /// Only an address that exists is told: the page starts without one.
    pub(crate) fn announce_address(&mut self) {
        if let Some(a) = &self.state.address.record {
            let shared = a.shared();
            self.emit(Event::Address(Some(shared)));
        }
    }
}
