//! Agent connection layer tests against the in-memory server, and ignored live tests.
mod address;
mod address_dr;
mod chat_bytes;
pub(crate) mod cli;
mod delivery;
mod handshake;
mod interop;
mod live;
mod restart;
mod support;
mod ws;
