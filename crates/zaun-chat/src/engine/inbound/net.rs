//! Offline: no relay of the user's is up; reported once each way (the old codec's `hostDisconnected` rule).

use zaun_chat_wire::Event;

use crate::engine::agent_port::AgentPort;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    /// A relay host came up or went away. When the user named their own relays, only those count:
    /// a peer's relay the agent closed when idle sends no Down and would stay "up" for good.
    pub(crate) fn on_host(&mut self, server: String, up: bool) {
        let own = &self.relays.own_hosts;
        if !own.is_empty() && !own.contains(&server) {
            return;
        }
        self.state.hosts.insert(server, up);
        let offline = !self.state.hosts.values().any(|up| *up);
        if offline != self.state.offline {
            self.state.offline = offline;
            self.emit(Event::Offline(offline));
        }
    }
}
