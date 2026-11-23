//! The agent event table: each event to its handler; a failure is logged (nobody asked for it).

use zaun_chat_wire::model::Delivery;

use crate::engine::agent_port::{AgentEvent, AgentPort};
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_agent(&mut self, event: AgentEvent) {
        let r = match event {
            AgentEvent::Invitation { conn, full, short } => self.on_invitation(&conn, full, short),
            AgentEvent::Conf { conn, conn_info } => self.on_conf(&conn, &conn_info),
            AgentEvent::Info { conn, conn_info } => self.on_info(&conn, &conn_info),
            AgentEvent::Con { conn } => self.on_con(&conn),
            AgentEvent::Msg { conn, msg_id, body } => {
                self.on_msg(&conn, msg_id, &body);
                Ok(())
            }
            AgentEvent::Sent { conn, msg_id } => self.on_delivery(&conn, msg_id, Delivery::Sent),
            AgentEvent::Rcvd { conn, msg_id } => {
                self.on_delivery(&conn, msg_id, Delivery::Delivered)
            }
            AgentEvent::Merr { conn, msg_id, err } => {
                self.log.line(format_args!(
                    "a message was not delivered: {}",
                    err.detail()
                ));
                self.on_delivery(&conn, msg_id, Delivery::Failed)
            }
            AgentEvent::Up { server } => {
                self.on_host(server, true);
                Ok(())
            }
            AgentEvent::Down { server } => {
                self.on_host(server, false);
                Ok(())
            }
            AgentEvent::Err { conn, err } => self.on_conn_error(&conn, &err),
            e @ (AgentEvent::Address { .. } | AgentEvent::Req { .. }) => self.on_address_event(e),
        };
        if let Err(e) = r {
            self.log
                .line(format_args!("an agent event failed: {}", e.detail));
        }
    }
}
