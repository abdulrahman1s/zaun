//! Shared fakes, fixtures and golden-file helpers for EasyPlay's tests.
//!
//! **Dev-dependency only** (MODULES §11): no non-test code may depend on this crate.
//!
//! Note for `easyplay-core`'s own unit tests: a `#[cfg(test)]` module inside `easyplay-core`
//! sees a different copy of the core types than this crate does, so use these fakes from
//! `crates/easyplay-core/tests/*.rs` integration tests (or from other crates), not from unit
//! tests inside core.

#![forbid(unsafe_code)]
// Test support code: panicking on broken fixtures is the desired behaviour.
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod bins;
mod env;
pub mod fakes;
pub mod fixtures;
pub mod golden;

pub use env::TestEnv;
pub use fakes::{
    CannedOutput, FakeClock, FakeHttpServer, FakeLauncher, FakeProc, FakeStart, MemFs, ROOT_DEVICE,
    Reply, ScriptedRunner, UnexpectedCommand, proc_entry,
};

#[doc(hidden)]
pub use insta;
