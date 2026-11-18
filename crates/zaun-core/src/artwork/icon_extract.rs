//! Icons embedded in executables (ART-003, GAME_IDENTITY §6).
//!
//! [`IconSource`] is the seam; [`PeIconSource`] is the real implementation over
//! [`identity::pe::PeInspector`](crate::identity::pe::PeInspector), which reads the file
//! through `fsx` with [`PeLimits`] (bounded reads, deadline, panic isolation) and builds a
//! single-image ICO from the largest icon. [`decode_icon`] turns those bytes into pixels.

use std::path::Path;
use std::sync::Arc;

use super::codec::{CodecError, ImageCodec, ImageLimits, RgbaImage};
use crate::error::Result;
use crate::identity::pe::{PeError, PeInspector, PeLimits};

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

/// No icons at all (loose non-PE files, tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoIcons;

impl IconSource for NoIcons {
    fn largest_icon(&self, _exe: &Path) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

/// [`IconSource`] backed by a [`PeInspector`]: the ICO bytes of the largest icon.
///
/// A file that is not a PE program (or not an executable at all) has no icon (`Ok(None)`);
/// a PE file that cannot be read within the limits is an error (`ZN-ID-002`), which callers
/// treat as "no icon" plus a log line.
#[derive(Clone)]
pub struct PeIconSource {
    inspector: Arc<dyn PeInspector>,
    limits: PeLimits,
}

impl PeIconSource {
    /// A source using `inspector` with the default [`PeLimits`], the icon cap lowered to
    /// [`ICON_LIMITS`]`.max_bytes`.
    pub fn new(inspector: Arc<dyn PeInspector>) -> Self {
        let limits = PeLimits {
            max_icon_bytes: ICON_LIMITS.max_bytes as usize,
            ..PeLimits::default()
        };
        Self { inspector, limits }
    }

    /// A source with explicit limits.
    pub fn with_limits(inspector: Arc<dyn PeInspector>, limits: PeLimits) -> Self {
        Self { inspector, limits }
    }
}

impl std::fmt::Debug for PeIconSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeIconSource")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl IconSource for PeIconSource {
    fn largest_icon(&self, exe: &Path) -> Result<Option<Vec<u8>>> {
        match self.inspector.largest_icon(exe, &self.limits) {
            Ok(icon) => Ok(icon.map(|i| i.ico)),
            Err(PeError::NotExecutable | PeError::NotPe(_)) => Ok(None),
            Err(e) => Err(crate::Error::from(e).with("path", exe.display())),
        }
    }
}

/// Decode an extracted icon with [`ICON_LIMITS`] (ICO: the largest entry).
pub fn decode_icon(codec: &dyn ImageCodec, bytes: &[u8]) -> Result<RgbaImage, CodecError> {
    codec.decode(bytes, &ICON_LIMITS)
}
