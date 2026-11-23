//! A sent message's delivery: Sending, then Sent, then Delivered by receipt, or Failed.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::Delivery;

use crate::engine::agent_port::{AgentMsgId, AgentPort, ConnId};
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{NS, keys};

/// How far along a delivery is; it only moves forward.
fn rank(d: Delivery) -> u8 {
    match d {
        Delivery::Sending | Delivery::Received => 0,
        Delivery::Sent => 1,
        Delivery::Delivered | Delivery::Failed => 2,
    }
}

impl<A: AgentPort> Engine<A> {
    /// The `snd/` index row goes with the receipt or the failure: nothing follows them.
    pub(crate) fn on_delivery(
        &mut self,
        conn: &ConnId,
        msg: AgentMsgId,
        to: Delivery,
    ) -> Result<()> {
        let Some(friend) = self.state.friend_of(conn) else {
            return Ok(());
        };
        let key = keys::sent(conn, msg);
        let Some(id) = self.get_index(&key)? else {
            return Ok(());
        };
        if rank(to) == 2 {
            self.store.delete(NS, &key)?;
        }
        let Some(mut item) = self.item(friend, id)? else {
            return Ok(());
        };
        if rank(to) <= rank(item.delivery) {
            return Ok(());
        }
        item.delivery = to;
        let m = self.change_item(friend, id, &item)?;
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }
}
