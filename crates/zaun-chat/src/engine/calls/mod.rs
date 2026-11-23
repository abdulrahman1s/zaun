//! Call and stream signalling (ADR-0035/0036): `x.call.*` both ways, the call key and the call lines (Commands.hs `APISendCallInvitation` …, Subscriber.hs `xCallInv` …).

mod inbound;
mod key;
mod line;
mod send;
mod state;
mod stream;
mod timer;
mod webrtc;

pub use state::CallState;

#[cfg(test)]
pub(crate) use key::{media_key, public_b64};
