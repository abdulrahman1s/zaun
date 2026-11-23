//! The native chat engine (ADR-0042): one thread owning state, store and agent, ported from
//! simplex-chat's `Library/Commands.hs` and `Library/Subscriber.hs` (AGPL-3.0-only).

pub mod agent_adapter;
mod agent_map;
pub mod agent_port;
mod chats;
pub mod clock;
pub mod error;
mod inbound;
mod items;
mod load;
pub mod log;
pub mod machine;
mod presets;
mod profile;
mod quote;
pub mod records;
mod requests;
pub mod run;
mod start;
pub mod state;
mod time;

#[cfg(test)]
mod tests;

pub use agent_adapter::NativeAgent;
pub use agent_port::{AgentEvent, AgentFailure, AgentMsgId, AgentPort, ConnId};
pub use clock::{Clock, FakeClock, SystemClock};
pub use machine::Engine;
