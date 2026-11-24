//! Zaun's Chat helper (ADR-0042): the simplex-client engine (AGPL-3.0-only), spoken to Zaun
//! over `zaun-chat-wire` on standard input and output.

#![forbid(unsafe_code)]

pub mod wire;

pub use wire::run;
