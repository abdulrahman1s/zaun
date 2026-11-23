//! Offline: every relay host the agent reported is down; reported once each way (codec rule).

use zaun_chat_wire::Event;

use crate::engine::agent_port::AgentPort;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_host(&mut self, server: String, up: bool) {
        self.state.hosts.insert(server, up);
        let offline = !self.state.hosts.values().any(|up| *up);
        if offline != self.state.offline {
            self.state.offline = offline;
            self.emit(Event::Offline(offline));
        }
    }
}
