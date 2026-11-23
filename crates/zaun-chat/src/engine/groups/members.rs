//! Members: the user first, then everyone still in the group (codec `parse_members`).

use zaun_chat_wire::Event;

use super::list::member_wire;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn members(&mut self, group: i64) -> Result<()> {
        let g = self.group(group)?;
        let others = self
            .state
            .members_of(group)
            .filter(|m| !m.status.gone())
            .map(member_wire);
        let members = std::iter::once(self.me_wire(&g)).chain(others).collect();
        self.emit(Event::Members { group, members });
        Ok(())
    }
}
