//! Engine rule tests against `FakeAgent` and `FakeClock`, named after the codec tests they port.

mod contact;
mod fake_agent;
mod handshake;
mod inbound;
mod local;
mod pipe;
mod restart;
mod send;
mod start;

use std::path::PathBuf;

use zaun_chat_wire::model::ChatMessage;
use zaun_chat_wire::{Event, Hello, Request};

use super::agent_port::{AgentEvent, ConnId};
use super::clock::FakeClock;
use super::log::Log;
use super::machine::Engine;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage as XMessage, encode_chat_message};
use crate::store::Store;
pub use fake_agent::{Call, FakeAgent};

pub const KEY: &str = "test key";

/// A scratch directory removed on drop.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Dir {
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).unwrap();
        let name: String = id.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("zaun-engine-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    pub fn hello(&self, name: &str) -> Hello {
        Hello::new(self.0.clone(), KEY.into(), name.into(), Vec::new())
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct T {
    pub e: Engine<FakeAgent>,
    pub clock: FakeClock,
    pub dir: Dir,
}

impl T {
    /// An engine for "alice", not started.
    pub fn new() -> T {
        let dir = Dir::new();
        let store = Store::open(&dir.0.join("chat.db"), KEY).unwrap();
        let clock = FakeClock::new(1_795_000_000_000);
        let e = Engine::new(
            store,
            FakeAgent::default(),
            Box::new(clock.clone()),
            Log::discard(),
        );
        T { e, clock, dir }
    }

    /// Started, with what the start put out taken.
    pub fn started() -> T {
        let mut t = T::new();
        t.e.start(&t.dir.hello("alice")).unwrap();
        t.out();
        t
    }

    pub fn out(&mut self) -> Vec<Event> {
        std::iter::from_fn(|| self.e.pop_out()).collect()
    }

    pub fn req(&mut self, r: Request) -> Vec<Event> {
        self.e.on_request(r);
        self.out()
    }

    pub fn agent(&mut self, ev: AgentEvent) -> Vec<Event> {
        self.e.agent.queued.push(ev);
        self.e.pump();
        self.out()
    }

    /// Someone named `name` answers a fresh invitation and connects: their friend id and conn.
    pub fn friend(&mut self, name: &str) -> (i64, ConnId) {
        self.req(Request::CreateInvite);
        let conn = self.e.state.inviting.unwrap();
        self.agent(AgentEvent::Invitation {
            conn,
            full: format!("simplex:/invitation#/?v=2-7&name={name}"),
            short: None,
        });
        self.agent(AgentEvent::Conf {
            conn,
            conn_info: x_info(name),
        });
        self.agent(AgentEvent::Con { conn });
        (self.e.state.friend_of(&conn).unwrap(), conn)
    }

    /// A message from the peer on `conn`, numbered `msg_id` by the agent.
    pub fn peer_says(&mut self, conn: &ConnId, msg_id: i64, event: ChatEvent) -> Vec<Event> {
        let msg = XMessage::new(event).unwrap();
        let body = encode_chat_message(&msg, false).unwrap();
        self.agent(AgentEvent::Msg {
            conn: *conn,
            msg_id,
            body,
        })
    }

    pub fn acks(&self) -> Vec<(i64, Option<Vec<u8>>)> {
        self.e
            .agent
            .calls
            .iter()
            .filter_map(|c| {
                if let Call::Ack {
                    msg_id, receipt, ..
                } = c
                {
                    Some((*msg_id, receipt.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
}

pub fn x_info(name: &str) -> Vec<u8> {
    let msg = XMessage::without_id(ChatEvent::Info {
        profile: Profile::new(name, ""),
        member_key: None,
    });
    msg.to_json().unwrap()
}

/// The messages of the only `Messages` or `Updated` event.
pub fn messages(out: &[Event]) -> Vec<ChatMessage> {
    out.iter()
        .flat_map(|e| {
            if let Event::Messages(m) | Event::Updated(m) = e {
                m.clone()
            } else {
                Vec::new()
            }
        })
        .collect()
}
