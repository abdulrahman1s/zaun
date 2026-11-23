//! Protocol versions and ranges (simplexmq Simplex/Messaging/Version.hs).
use super::{
    Decode, Decoder, Encode,
    error::{DecodeError, EncodeError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u16);

/// Invariant: `min <= max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionRange {
    min: Version,
    max: Version,
}

impl VersionRange {
    /// `safeVersionRange`.
    #[must_use]
    pub fn new(min: Version, max: Version) -> Option<Self> {
        (min <= max).then_some(Self { min, max })
    }

    #[must_use]
    pub fn single(v: Version) -> Self {
        Self { min: v, max: v }
    }

    #[must_use]
    pub fn min(&self) -> Version {
        self.min
    }

    #[must_use]
    pub fn max(&self) -> Version {
        self.max
    }

    /// `isCompatible`.
    #[must_use]
    pub fn contains(&self, v: Version) -> bool {
        self.min <= v && v <= self.max
    }

    /// `isCompatibleRange`.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.min <= other.max && other.min <= self.max
    }

    /// `compatibleVRange`: the intersection.
    #[must_use]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        Self::new(self.min.max(other.min), self.max.min(other.max))
    }

    /// `compatibleVersion`: the highest version both sides speak.
    #[must_use]
    pub fn highest_common(&self, other: &Self) -> Option<Version> {
        self.overlaps(other).then(|| self.max.min(other.max))
    }

    /// `compatibleVRange'`: this range capped at `v`.
    #[must_use]
    pub fn capped(&self, v: Version) -> Option<Self> {
        if v <= self.max {
            Self::new(self.min, v)
        } else {
            None
        }
    }
}

impl Encode for Version {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

impl Decode for Version {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        u16::decode(d).map(Version)
    }
}

impl Encode for VersionRange {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.min, self.max).encode(out)
    }
}

impl Decode for VersionRange {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (min, max) = <(Version, Version)>::decode(d)?;
        Self::new(min, max).ok_or(DecodeError::Invalid("version range"))
    }
}
