//! Message delivery: the per-queue layer, queued sends with retries, receiving, acks and receipts (Agent.hs, Agent/Client.hs; simplexmq 7.1.4, AGPL-3.0-only).
mod client_msg;
mod enqueue;
mod message;
mod outcome;
mod receipts;
mod receive;
mod retry;
mod seal;
mod worker;

pub use client_msg::{ClientMessage, ClientMsgEnvelope, PrivHeader};
pub(crate) use retry::{Retry, RetryMode};
pub(crate) use seal::{Opened, seal};
