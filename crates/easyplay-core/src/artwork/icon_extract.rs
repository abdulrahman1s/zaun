//! Icons embedded in executables (ART-003, GAME_IDENTITY §6).
//!
//! The PE-backed implementation of [`IconSource`] arrives with `identity::pe`
//! (`PeInspector`, IDENT-002); this module defines the seam and the decoding step, so callers
//! and tests can already use any source of icon bytes.

use std::path::Path;

use super::codec::{CodecError, ImageCodec, ImageLimits, RgbaImage};
use crate::error::Result;

/// Limits for embedded icons: an icon group's largest image is at most 1024² in practice;
/// anything bigger is ignored (THREAT_MODEL T-18 "compatibility cost").
pub const ICON_LIMITS: ImageLimits = ImageLimits {
    max_bytes: 4 * 1024 * 1024,
    max_pixels: 1024 * 1024,
    max_side: 1024,
};

/// Something that can produce the largest icon of an executable as encoded image bytes
/// (an ICO container, or a PNG/BMP of the single largest entry). Implementations read only
/// through `fsx` with their own limits and never follow a symlink in the last component.
pub trait IconSource: Send + Sync {
    /// The largest icon of `exe`; `Ok(None)` when it has none.
    fn largest_icon(&self, exe: &Path) -> Result<Option<Vec<u8>>>;
}

/// No icons at all (until the PE-backed source exists, and for loose non-PE files).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoIcons;

impl IconSource for NoIcons {
    fn largest_icon(&self, _exe: &Path) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

/// Decode an extracted icon with [`ICON_LIMITS`] (ICO: the largest entry).
pub fn decode_icon(codec: &dyn ImageCodec, bytes: &[u8]) -> Result<RgbaImage, CodecError> {
    codec.decode(bytes, &ICON_LIMITS)
}
