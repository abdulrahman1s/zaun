//! Chat (ADR-0033): end-to-end encrypted chat with friends and groups over SimpleX.
//!
//! - [`engine`]: the seam the service uses ([`Engine`], [`EngineFactory`], [`Request`],
//!   [`ChatEvent`]), in the protocol-neutral types of `model::chat`.
//! - [`simplex`]: the one engine, the `zaun-chat` helper over pipes.
//! - [`key`]: the database key at rest.
//! - [`picture`]: profile pictures, checked and made (ADR-0038).
//! - [`fake`]: an engine for tests.
//! - [`media`]: voice calls (ADR-0035): the media seam, ICE servers, call signals.
//! - [`overlay`]: the in-game overlay's protocol, snapshot and process (ADR-0037).
//!
//! Depends on: `model`, `error`, `fsx`, `clock`, `session` (to start the helper).

pub mod engine;
pub mod fake;
pub mod key;
pub mod media;
pub mod native;
pub mod overlay;
pub mod picture;
pub mod simplex;

pub use engine::{
    ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, FileKind, Request, RequestKind,
};
pub use native::NativeFactory;
pub use simplex::SimplexFactory;

/// The reactions the chat engine carries, in display form (fully qualified emoji). The
/// engine refuses every other emoji, so the real service publishes this list as
/// `ChatSnapshot::reaction_emoji` and the page offers only these.
pub const REACTION_EMOJI: [&str; 8] = ["👍", "👎", "😀", "😂", "😢", "❤️", "🚀", "✅"];

/// The largest picture a friend's app fetches by itself (1 MiB); a larger one, and every
/// video and other file, is offered and fetched when asked (`ChatCommand::ReceiveFile`).
pub const MAX_FILE: u64 = 1024 * 1024;

/// The largest file sent (1 GiB, the engine's file-server limit).
pub const MAX_SEND: u64 = 1024 * 1024 * 1024;
