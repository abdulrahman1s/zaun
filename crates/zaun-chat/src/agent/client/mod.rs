//! The agent connection layer: duplex connections over SMP queues for the engine thread (simplexmq 7.1.4 Agent.hs, AGPL-3.0-only).
//!
//! `AgentClient` is owned by the engine thread and never blocks. Results come back as `AgentEvent`s from
//! `drain_events`, which the engine calls after each call into the client. SMP events, including the answers
//! to the client's commands, arrive on the channel given to `AgentClient::new`; the engine passes each to
//! `on_smp`. `on_tick` runs retries; its `now` and `next_wake` are Unix ms of the same wall clock the client
//! was given (`WallClock`), never monotonic time. `subscribe_all` loads the store and must run first.
mod agent_client;
mod clock;
mod config;
mod conn;
mod dispatch;
mod event;
mod ids;
mod ops;
mod save;
mod servers;
mod temporary;

pub use agent_client::AgentClient;
pub use clock::{SystemWallClock, WallClock};
pub(crate) use config::ms;
pub use config::{AgentConfig, DeliveryConfig, RetryInterval};
pub use event::{AgentEvent, RcptStatus, Receipt};
pub use ids::{AgentMsgId, ConnId};
pub(crate) use ops::OpKind;
pub(crate) use temporary::is_temporary;
