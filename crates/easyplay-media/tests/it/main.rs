//! The call media's tests in one binary (ADR-0035). `relay` and `call_e2e` are ignored: they
//! need a local TURN server and the chat engine (a local recipe kept by the maintainer).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

mod call_e2e;
mod common;
mod loopback;
#[cfg(feature = "hwenc")]
mod nvenc;
#[cfg(feature = "portal")]
mod portal;
mod pulse;
mod relay;
mod stream_e2e;
pub(crate) mod stream_loopback;
