//! What arrives from the agent: the handshake, messages and their `x.*` events, delivery, relays.

mod agent;
mod contact;
mod delivery;
mod handshake;
mod message;
mod net;
mod sync;
mod x_msg;

pub(crate) use message::asks_receipt;
