//! E2E ratchet versions: 1 binary encoding, 2 KDF in X3DH, 3 PQDR; 7.1.4 speaks only 3 (Ratchet.hs `minSupportedE2EEncryptVersion`, `supportedE2EEncryptVRange`).
use crate::encoding::{Version, VersionRange};

pub const MIN_SUPPORTED_E2E_VERSION: Version = Version(3);
pub const CURRENT_E2E_VERSION: Version = Version(3);

#[must_use]
pub fn supported_e2e_range() -> VersionRange {
    VersionRange::single(CURRENT_E2E_VERSION)
}

/// `compatibleVersion` against our range: the version to run, or none when the peer is out of range.
#[must_use]
pub fn compatible_e2e_version(peer: &VersionRange) -> Option<Version> {
    peer.highest_common(&supported_e2e_range())
}

/// `adjustE2EVRange`: a range from a link is lifted to start at the minimum we support.
#[must_use]
pub fn adjust_e2e_range(vr: VersionRange) -> VersionRange {
    let min = vr.min().max(MIN_SUPPORTED_E2E_VERSION);
    VersionRange::new(min, vr.max().max(min)).unwrap_or(vr)
}

/// The versions a ratchet sends and accepts (Ratchet.hs `RatchetVersions`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RatchetVersions {
    pub current: Version,
    pub max_supported: Version,
}

impl RatchetVersions {
    #[must_use]
    pub fn new(current: Version) -> Self {
        Self {
            current,
            max_supported: CURRENT_E2E_VERSION,
        }
    }
}
