//! One call's peer connection on its own thread (ADR-0035): str0m driven by our UDP socket
//! and TURN relays, Opus in and out.

mod frame_crypto;
mod gather;
mod inbox;
mod mids;
mod relay;
mod run;
pub(crate) mod video;

pub(crate) use inbox::Inbox;
pub(crate) use run::{Options, spawn};
