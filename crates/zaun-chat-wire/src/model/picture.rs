//! A picture on the wire: its bytes and what they are.

use serde::{Deserialize, Serialize};

use super::bytes;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picture {
    #[serde(with = "bytes")]
    pub bytes: Vec<u8>,
    /// `image/jpeg` or `image/png`.
    pub mime: String,
}

impl std::fmt::Debug for Picture {
    // Never the bytes: logs print profiles.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture({} bytes, {})", self.bytes.len(), self.mime)
    }
}
