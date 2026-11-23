//! An agent that records what the engine asked and hands back the events a test queues.

use crate::agent::links::ConnectionLink;
use crate::engine::agent_port::{AgentEvent, AgentFailure, AgentMsgId, AgentPort, ConnId};
use crate::store::Store;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    CreateInvitation,
    Join {
        link: String,
        conn_info: Vec<u8>,
    },
    Allow {
        conn: ConnId,
        conn_info: Vec<u8>,
    },
    Send {
        conn: ConnId,
        body: Vec<u8>,
    },
    Ack {
        conn: ConnId,
        msg_id: AgentMsgId,
        receipt: Option<Vec<u8>>,
    },
    Delete {
        conn: ConnId,
    },
    SubscribeAll,
}

#[derive(Debug, Default)]
pub struct FakeAgent {
    pub calls: Vec<Call>,
    pub queued: Vec<AgentEvent>,
    /// The next call that can fail fails with this.
    pub fail_next: Option<AgentFailure>,
    /// Events `subscribe_all` queues.
    pub on_subscribe: Vec<AgentEvent>,
    conns: i64,
    msgs: AgentMsgId,
}

impl FakeAgent {
    fn conn(&mut self) -> Result<ConnId, AgentFailure> {
        self.fail_next.take().map_or(Ok(()), Err)?;
        self.conns += 1;
        Ok(ConnId(self.conns))
    }

    pub fn sent(&self) -> Vec<(ConnId, Vec<u8>)> {
        self.calls
            .iter()
            .filter_map(|c| {
                if let Call::Send { conn, body } = c {
                    Some((*conn, body.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl AgentPort for FakeAgent {
    type Smp = AgentEvent;

    fn create_invitation(
        &mut self,
        _: &mut Store,
        _: bool,
        _: bool,
        _: Vec<u8>,
    ) -> Result<ConnId, AgentFailure> {
        self.calls.push(Call::CreateInvitation);
        self.conn()
    }

    fn join(
        &mut self,
        _: &mut Store,
        link: &ConnectionLink,
        conn_info: Vec<u8>,
        _: bool,
    ) -> Result<ConnId, AgentFailure> {
        self.calls.push(Call::Join {
            link: link.to_uri(),
            conn_info,
        });
        self.conn()
    }

    fn allow(
        &mut self,
        _: &mut Store,
        conn: &ConnId,
        conn_info: Vec<u8>,
    ) -> Result<(), AgentFailure> {
        self.calls.push(Call::Allow {
            conn: *conn,
            conn_info,
        });
        Ok(())
    }

    fn send(
        &mut self,
        _: &mut Store,
        conn: &ConnId,
        body: Vec<u8>,
    ) -> Result<AgentMsgId, AgentFailure> {
        self.calls.push(Call::Send { conn: *conn, body });
        self.fail_next.take().map_or(Ok(()), Err)?;
        self.msgs += 1;
        Ok(self.msgs)
    }

    fn ack(
        &mut self,
        _: &mut Store,
        conn: &ConnId,
        msg_id: AgentMsgId,
        receipt: Option<Vec<u8>>,
    ) -> Result<(), AgentFailure> {
        self.calls.push(Call::Ack {
            conn: *conn,
            msg_id,
            receipt,
        });
        Ok(())
    }

    fn delete(&mut self, _: &mut Store, conn: &ConnId) -> Result<(), AgentFailure> {
        self.calls.push(Call::Delete { conn: *conn });
        Ok(())
    }

    fn subscribe_all(&mut self, _: &mut Store) -> Result<(), AgentFailure> {
        self.calls.push(Call::SubscribeAll);
        self.queued.append(&mut self.on_subscribe);
        Ok(())
    }

    /// The test's SMP "events" are agent events, queued as the real agent would.
    fn on_smp(&mut self, _: &mut Store, event: AgentEvent) {
        self.queued.push(event);
    }

    fn on_tick(&mut self, _: &mut Store, _: i64) {}

    fn next_wake(&self) -> Option<i64> {
        None
    }

    fn close(&mut self) {}

    fn drain_events(&mut self) -> Vec<AgentEvent> {
        std::mem::take(&mut self.queued)
    }
}
