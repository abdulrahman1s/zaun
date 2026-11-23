//! The pipe vocabulary between Zaun's core and the chat helper `zaun-chat` (ADR-0033).

pub mod event;
pub mod features;
pub mod frame;
pub mod kind;
pub mod model;
pub mod request;

pub use event::Event;
pub use features::{REACTION_EMOJI, features_with, has_file_servers};
pub use frame::{EvLine, Hello, ReqLine, WireError, decode_line, encode_line};
pub use kind::{FileKind, RequestKind};
pub use request::Request;

/// The protocol version in [`Hello::v`].
pub const VERSION: u32 = 2;
