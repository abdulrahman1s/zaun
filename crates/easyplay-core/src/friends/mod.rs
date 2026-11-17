//! Friends (ADR-0033): end-to-end encrypted chat with friends and groups over SimpleX.
//!
//! - [`engine`]: the seam the service uses ([`Engine`], [`EngineFactory`], [`Request`],
//!   [`FriendsEvent`]), in the protocol-neutral types of `model::friends`.
//! - [`simplex`]: the one engine, the `easyplay-friends` helper over pipes.
//! - [`key`]: the database key at rest.
//! - [`fake`]: an engine for tests.
//!
//! Depends on: `model`, `error`, `fsx`, `clock`, `session` (to start the helper).

pub mod engine;
pub mod fake;
pub mod key;
pub mod simplex;

pub use engine::{
    Engine, EngineConfig, EngineFactory, EventSink, FriendsEvent, Request, RequestKind,
};
pub use simplex::SimplexFactory;
