//! Our request was accepted: the owner's profile arrives as `CONF` on the connection we made for it, and we answer with ours (Subscriber.hs `processDirectMessage` CONF with `x.info`).

use crate::engine::agent_port::{AgentPort, ConnId};
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::{conn_info, received_profile};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_owner_conf(&mut self, friend: i64, conn: &ConnId, info: &[u8]) -> Result<()> {
        let profile = received_profile(info)
            .ok_or_else(|| fail(Code::RequestFailed, "conn-info without a profile"))?;
        self.on_x_info(friend, &profile)?;
        let ours = conn_info(&self.state.user)?;
        self.agent.allow(&mut self.store, conn, ours)?;
        Ok(())
    }
}
