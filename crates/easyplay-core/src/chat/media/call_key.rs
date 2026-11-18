//! The key the chat engine agrees for a call (ADR-0035 §4): with it, each voice frame is
//! encrypted once more inside the media (AES-256-GCM), as the official apps do.
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};

/// 32 bytes from the engine's `sharedKey` (base64url). Never logged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CallKey(pub [u8; 32]);

impl CallKey {
    /// The engine's text form, padded or not; `None` for anything that is not 32 bytes.
    pub fn from_base64url(text: &str) -> Option<Self> {
        let bytes = URL_SAFE
            .decode(text)
            .or_else(|_| URL_SAFE_NO_PAD.decode(text))
            .ok()?;
        Some(Self(bytes.try_into().ok()?))
    }
}

impl fmt::Debug for CallKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CallKey(<set>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engines_key_reads_padded_or_not_and_only_at_32_bytes() {
        let padded = URL_SAFE.encode([7u8; 32]);
        assert_eq!(CallKey::from_base64url(&padded), Some(CallKey([7; 32])));
        let bare = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert_eq!(CallKey::from_base64url(&bare), Some(CallKey([7; 32])));
        assert_eq!(CallKey::from_base64url(&URL_SAFE.encode([7u8; 16])), None);
        assert_eq!(CallKey::from_base64url("not base64!"), None);
        assert_eq!(format!("{:?}", CallKey([7; 32])), "CallKey(<set>)");
    }
}
