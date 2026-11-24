//! The helper end to end: its pipes, its vocabulary against `zaun-chat-wire`, and (ignored) two
//! helpers through a real smp-server. The engine's own tests live in simplex-client.
#![allow(clippy::unwrap_used, reason = "tests")]

mod contract;
mod engine_live;
mod engine_pipe;
#[path = "../../../zaun-chat-wire/tests/it/samples.rs"]
mod samples;
