//! The native SimpleX engine run by the Chat helper (ADR-0042): the protocols ported from
//! simplexmq and simplex-chat (AGPL-3.0-only), spoken to Zaun over `zaun-chat-wire`.

#![forbid(unsafe_code)]

pub mod agent;
pub mod chat;
pub mod crypto;
pub mod encoding;
pub mod engine;
pub mod h2;
pub mod net;
pub mod ratchet;
pub mod smp;
pub mod store;
pub mod tls;
pub mod wire;
pub mod xftp;

pub use wire::run;
