//! Tags (DATA_MODEL §6 `tags`, GAME_LIBRARY §7).

use serde::{Deserialize, Serialize};

use super::{Problem, ValidationError};
use crate::clock::Timestamp;
use crate::consts::TAG_COLORS;
use crate::ids::{TagId, TagName};

/// A tag colour index `0..8` into the theme's tag palette.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(try_from = "u8", into = "u8")]
pub struct TagColor(u8);

impl TagColor {
    /// Validate a colour index.
    pub fn new(index: u8) -> Result<Self, ValidationError> {
        if index < TAG_COLORS {
            Ok(Self(index))
        } else {
            Err(ValidationError::new("tag.color", Problem::OutOfRange))
        }
    }

    /// The index.
    pub const fn index(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for TagColor {
    type Error = ValidationError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        Self::new(v)
    }
}

impl From<TagColor> for u8 {
    fn from(c: TagColor) -> Self {
        c.0
    }
}

/// A tag row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// Identifier.
    pub id: TagId,
    /// Name (unique, case-insensitive).
    pub name: TagName,
    /// Colour.
    pub color: TagColor,
    /// Creation time.
    pub created_at: Timestamp,
}
