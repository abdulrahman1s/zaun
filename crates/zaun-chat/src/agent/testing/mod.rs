//! Test support for the agent: an in-memory SMP server and a driver that runs agents as the engine loop would.
mod commands;
mod connector;
mod node;
mod parse;
mod pipe;
mod queues;
mod server;

pub use connector::FakeConnector;
pub use node::{Dir, Node, TestClock, pump};
pub use queues::new_queue;
pub use server::FakeSmp;
