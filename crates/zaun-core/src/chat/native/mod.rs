//! The native chat engine (ADR-0042): `zaun-chat` run under Bubblewrap, spoken to in
//! `zaun-chat-wire` lines over its standard input and output.
//!
//! - [`factory`]: [`NativeFactory`], which starts the helper.
//! - [`command`]: the sandbox's command line and environment.
//! - [`pipe`]: the writer and reader threads; [`engine`]: the running helper.
//! - [`to_wire`], [`from_wire`]: core's types to the wire's and back.
//! - [`status`]: what the helper's exit status means.

mod command;
mod engine;
mod factory;
mod from_wire;
mod pipe;
mod status;
#[cfg(test)]
mod tests;
mod to_wire;

pub use factory::{LOG_NAME, NativeFactory};
