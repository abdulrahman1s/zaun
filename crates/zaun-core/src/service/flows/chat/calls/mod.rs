//! Voice calls (ADR-0035): one call at a time, signalled through the chat engine, its sound
//! carried by the call media (`ServiceDeps::media`).

mod answer;
mod controls;
mod decline;
mod devices;
mod hang_up;
mod incoming;
mod media;
mod mic_test;
mod place;
mod signal;
mod state;
mod timeout;

pub(crate) use state::{CallDone, CallFlow};
