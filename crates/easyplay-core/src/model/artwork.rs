//! Artwork references (DATA_MODEL §6 `artwork`, ADR-0009).

use serde::{Deserialize, Serialize};

use super::Sha256;
use crate::clock::Timestamp;
use crate::ids::GameId;

/// Which picture of a game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtSlot {
    /// Portrait cover (grid cards).
    Cover,
    /// Square icon (list rows).
    Icon,
    /// Wide banner (game page).
    Hero,
}

impl ArtSlot {
    /// Every slot.
    pub const ALL: [ArtSlot; 3] = [ArtSlot::Cover, ArtSlot::Icon, ArtSlot::Hero];

    /// Text form for `artwork.slot` and file names.
    pub const fn as_str(self) -> &'static str {
        match self {
            ArtSlot::Cover => "cover",
            ArtSlot::Icon => "icon",
            ArtSlot::Hero => "hero",
        }
    }
}

/// Where a stored picture came from (`artwork.source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtOrigin {
    /// A user-chosen image file.
    File,
    /// The icon embedded in an executable.
    ExecutableIcon,
    /// Generated placeholder.
    Generated,
}

impl ArtOrigin {
    /// Text form for `artwork.source`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ArtOrigin::File => "file",
            ArtOrigin::ExecutableIcon => "executable_icon",
            ArtOrigin::Generated => "generated",
        }
    }
}

/// An `artwork` row: a normalised picture stored under `artwork/<game_id>/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkRef {
    /// Game.
    pub game_id: GameId,
    /// Slot.
    pub slot: ArtSlot,
    /// File name inside `artwork/<game_id>/`.
    pub file: String,
    /// SHA-256 of the stored file.
    #[serde(with = "super::executable::hex32")]
    pub hash: Sha256,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Origin.
    pub source: ArtOrigin,
    /// Last change.
    pub updated_at: Timestamp,
}
