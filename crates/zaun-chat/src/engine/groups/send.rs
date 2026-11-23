//! Sending to a group's members: one, or every member reachable now (Internal.hs `sendGroupMessage`, simplified; group connections are PQ off).

use crate::chat::{ChatMessage, encode_chat_message};
use crate::engine::agent_port::{AgentMsgId, AgentPort, ConnId};
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn send_to_member(
        &mut self,
        group: i64,
        member: i64,
        msg: &ChatMessage,
    ) -> Result<(ConnId, AgentMsgId)> {
        let conn = self
            .member(group, member)?
            .reachable()
            .ok_or_else(|| fail(Code::RequestFailed, "member not connected"))?;
        let body = encode_chat_message(msg, false)?;
        Ok((conn, self.agent.send(&mut self.store, &conn, body)?))
    }

    /// To every member with a working connection but `except`; one that fails is logged and
    /// left out. What was sent: (member, connection, agent message id).
    pub(crate) fn send_to_group(
        &mut self,
        group: i64,
        msg: &ChatMessage,
        except: Option<i64>,
    ) -> Result<Vec<(i64, ConnId, AgentMsgId)>> {
        let body = encode_chat_message(msg, false)?;
        let to: Vec<(i64, ConnId)> = self
            .state
            .members_of(group)
            .filter(|m| Some(m.id) != except)
            .filter_map(|m| Some((m.id, m.reachable()?)))
            .collect();
        let mut sent = Vec::with_capacity(to.len());
        for (member, conn) in to {
            match self.agent.send(&mut self.store, &conn, body.clone()) {
                Ok(id) => sent.push((member, conn, id)),
                Err(e) => self
                    .log
                    .line(format_args!("a group message was not sent: {}", e.detail())),
            }
        }
        Ok(sent)
    }
}
