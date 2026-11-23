//! Members connected to each other (`x.grp.mem.con`): the host stops forwarding between them (Subscriber.hs `xGrpMemCon`).

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::MemberRelation;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_members_connected(&mut self, group: i64, a: i64, b: i64) -> Result<()> {
        self.relate(group, a, b, MemberRelation::Connected)
    }
}
