//! Test doubles for TLS: an in-memory socket, certificates and a fault-injecting TLS 1.3 server.

mod app;
mod cert;
mod client_hello;
mod flight;
mod options;
mod pipe;
mod server;

pub(super) use cert::make_cert;
pub(super) use options::{MockOpts, MockResult};
pub(super) use pipe::Pipe;
pub(super) use server::spawn_server;
