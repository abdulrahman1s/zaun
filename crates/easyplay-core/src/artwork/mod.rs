//! Import, normalise, store and thumbnail images; Steam CDN artwork (opt-in).
//!
//! Layer 5. See MODULES §6, ADR-0009, ADR-0018 and LOGIC_SPECS §14.
//!
//! | File | Role |
//! |---|---|
//! | [`codec`] | `ImageCodec`: sniffing, limits, panic-isolated decode, encode |
//! | [`import`] | geometry per slot, dominant colour, content-addressed store, row upsert |
//! | [`icon_extract`] | `IconSource` seam for executable icons (PE source: IDENT-002) |
//! | [`thumbs`] | fixed-size thumbnails, `ThumbService` on the job pool |
//! | [`store`] | file names, sweeping unreferenced files, broken-row repair |
//! | [`placeholder`] | deterministic generated covers |
//! | [`steam`] | Steam app ID chain, CDN address list, fetch + import |
//! | [`http`] | the only HTTP client in the codebase |
//!
//! Nothing here reaches the network unless `artwork.online = "steam"`, and then only through
//! [`http`].

pub mod codec;
pub mod http;
pub mod icon_extract;
pub mod import;
pub mod placeholder;
pub mod steam;
pub mod store;
pub mod thumbs;

use std::path::PathBuf;

pub use codec::{
    CodecError, ImageCodec, ImageCrateCodec, ImageFormat, ImageInfo, ImageLimits, RgbaImage, sniff,
};
pub use icon_extract::{IconSource, NoIcons};
pub use import::{ImportDeps, ImportInput, Imported, Normalised, import, normalise};
pub use placeholder::{Placeholder, placeholder_for};
pub use steam::{Resolution, SteamAppIdSource};
pub use thumbs::{ArtIndex, ThumbKey, ThumbPixels, ThumbResult, ThumbService, ThumbSize};

use crate::ids::ExecutableId;
use crate::model::ArtOrigin;

/// Where a picture for a slot is to come from (MODULES §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtSource {
    /// A user-chosen image file.
    File(PathBuf),
    /// The icon embedded in one of the game's executables.
    ExecutableIcon(ExecutableId),
    /// The Steam CDN, for this app ID.
    SteamCdn(u32),
    /// The generated placeholder.
    Generated,
}

impl ArtSource {
    /// The `artwork.source` value a picture from here is stored with.
    pub const fn origin(&self) -> ArtOrigin {
        match self {
            ArtSource::File(_) => ArtOrigin::File,
            ArtSource::ExecutableIcon(_) => ArtOrigin::ExecutableIcon,
            ArtSource::SteamCdn(_) => ArtOrigin::SteamCdn,
            ArtSource::Generated => ArtOrigin::Generated,
        }
    }
}
