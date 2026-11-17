//! Chat (ADR-0033): end-to-end encrypted chat with friends and groups over SimpleX.
//!
//! - [`engine`]: the seam the service uses ([`Engine`], [`EngineFactory`], [`Request`],
//!   [`ChatEvent`]), in the protocol-neutral types of `model::chat`.
//! - [`simplex`]: the one engine, the `easyplay-chat` helper over pipes.
//! - [`key`]: the database key at rest.
//! - [`fake`]: an engine for tests.
//!
//! Depends on: `model`, `error`, `fsx`, `clock`, `session` (to start the helper).

pub mod engine;
pub mod fake;
pub mod key;
pub mod simplex;

pub use engine::{ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, Request, RequestKind};
pub use simplex::SimplexFactory;
