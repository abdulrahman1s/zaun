//! The `x.*` events of a chat message: kinds, typed params, JSON in and out (Protocol.hs `ChatMsgEvent`).
mod chunk;
mod decode;
mod encode;
mod flags;
mod kind;
mod kind_of;
mod types;

pub(crate) use chunk::{encode_chunk, parse_chunk};
pub(crate) use decode::decode;
pub(crate) use encode::encode;
pub use kind::EventKind;
pub use types::{ChatEvent, FileChunk};
