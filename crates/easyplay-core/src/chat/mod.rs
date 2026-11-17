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

pub use engine::{
    ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, FileKind, Request, RequestKind,
};
pub use simplex::SimplexFactory;

/// The reactions the chat engine carries, in display form (fully qualified emoji). The
/// engine refuses every other emoji, so the real service publishes this list as
/// `ChatSnapshot::reaction_emoji` and the page offers only these.
pub const REACTION_EMOJI: [&str; 8] = ["👍", "👎", "😀", "😂", "😢", "❤️", "🚀", "✅"];

/// The largest picture or GIF sent or received without asking (1 MiB): a friend's app
/// fetches a picture up to this size by itself, so a larger one would never show.
pub const MAX_FILE: u64 = 1024 * 1024;
