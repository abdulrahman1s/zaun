//! One element of a group message: a forward (`>`) goes to its author, a signed one (`/`) is checked first, and an admin passes on what it took (Subscriber.hs `processAChatMsg`, `withVerifiedMsg`, `xGrpMsgForward`, `createDeliveryTasks`).

use crate::chat::element::ParsedMsg;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;

use super::inbound::GroupCtx;

impl<A: AgentPort> Engine<A> {
    /// Each element passed on goes in a batch of its own: the elements of one message are
    /// not gathered.
    pub(crate) fn on_group_element(&mut self, group: i64, sender: i64, p: ParsedMsg) -> Result<()> {
        if let Some(el) = self.take_element(group, sender, p)? {
            self.forward_elements(group, sender, &[el])?;
        }
        Ok(())
    }

    /// Handles one element: what an admin passes on for it, if anything. A forward is never
    /// passed on again, and neither is an event its handler refused (an `Err`).
    pub(crate) fn take_element(
        &mut self,
        group: i64,
        sender: i64,
        p: ParsedMsg,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(fwd) = &p.forward {
            self.on_forwarded(group, sender, fwd, p.signed.as_ref(), p.message)?;
            return Ok(None);
        }
        self.check_signature(group, sender, p.signed.as_ref())?;
        let relay = self.relay_element(group, sender, &p);
        self.on_group_event(GroupCtx::direct(group, sender), p.message)?;
        Ok(relay)
    }
}
