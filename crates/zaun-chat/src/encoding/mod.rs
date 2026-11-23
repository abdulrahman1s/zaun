//! The binary encoding of SimpleX protocols (ADR-0042). Ported from simplexmq src/Simplex/Messaging/Encoding.hs and nook src/sx/encoding.ts (AGPL-3.0-only).

mod base64;
mod bytes;
mod containers;
mod decoder;
mod error;
mod prim;
mod time;
mod traits;
mod tuples;
mod version;

pub use base64::{b64_decode, b64_encode};
pub use bytes::{Large, Tail};
pub use containers::{List, NonEmpty};
pub use decoder::Decoder;
pub use error::{DecodeError, EncodeError};
pub use time::SysTime;
pub use traits::{Decode, Encode};
pub use version::{Version, VersionRange};
