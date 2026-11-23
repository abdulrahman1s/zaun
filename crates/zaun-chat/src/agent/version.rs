//! Agent, SMP client and E2E version constants and their feature gates (Agent/Protocol.hs, Protocol.hs, Crypto/Ratchet.hs; simplexmq 7.1.4, AGPL-3.0-only).
use crate::encoding::{Version, VersionRange};

pub const MIN_SUPPORTED_AGENT: Version = Version(6);
/// Initialise the ratchet on processing the confirmation.
pub const RATCHET_ON_CONF_AGENT: Version = Version(7);
/// Agent RPC, fast queue rotation (`QEND`) and ratchet-encrypted contact requests.
pub const RPC_ADDRESS_AGENT: Version = Version(8);
pub const CURRENT_AGENT: Version = Version(8);

pub const INITIAL_CLIENT: Version = Version(1);
pub const SRV_HOSTNAMES_CLIENT: Version = Version(2);
pub const SND_AUTH_KEY_CLIENT: Version = Version(3);
pub const SHORT_LINKS_CLIENT: Version = Version(4);
pub const CURRENT_CLIENT: Version = Version(4);

pub const MIN_SUPPORTED_E2E: Version = Version(3);
pub const CURRENT_E2E: Version = Version(3);

/// `supportedSMPAgentVRange` (6-8).
#[must_use]
pub fn supported_agent_range() -> VersionRange {
    range(MIN_SUPPORTED_AGENT, CURRENT_AGENT)
}

/// `supportedSMPClientVRange` (1-4).
#[must_use]
pub fn supported_client_range() -> VersionRange {
    range(INITIAL_CLIENT, CURRENT_CLIENT)
}

/// `supportedE2EEncryptVRange` (3-3).
#[must_use]
pub fn supported_e2e_range() -> VersionRange {
    range(MIN_SUPPORTED_E2E, CURRENT_E2E)
}

fn range(min: Version, max: Version) -> VersionRange {
    VersionRange::new(min, max).unwrap_or_else(|| VersionRange::single(min))
}

/// `minSMPClientVersion`: the lowest SMP client version a link for this agent version may offer.
#[must_use]
pub fn min_client_version(agent: Version) -> Version {
    if agent >= RPC_ADDRESS_AGENT {
        SHORT_LINKS_CLIENT
    } else if agent >= RATCHET_ON_CONF_AGENT {
        SND_AUTH_KEY_CLIENT
    } else {
        INITIAL_CLIENT
    }
}

/// v8 rotates a queue with `QADD` then `QEND`; older peers use `QKEY`, `QUSE` and `QTEST`.
#[must_use]
pub fn fast_rotation(agent: Version) -> bool {
    agent >= RPC_ADDRESS_AGENT
}

/// A contact address that publishes ratchet keys is joined with an `AgentContactRequest`.
#[must_use]
pub fn ratchet_contact_request(agent: Version) -> bool {
    agent >= RPC_ADDRESS_AGENT
}

/// The strEncode of a range: `6` or `6-8`.
#[must_use]
pub fn range_str(vr: &VersionRange) -> String {
    if vr.min() == vr.max() {
        vr.min().0.to_string()
    } else {
        format!("{}-{}", vr.min().0, vr.max().0)
    }
}

pub fn parse_range(s: &str) -> super::error::Result<VersionRange> {
    let num = |t: &str| {
        t.parse::<u16>()
            .map(Version)
            .map_err(|_| super::AgentError::Parse("version range"))
    };
    let (min, max) = match s.split_once('-') {
        Some((a, b)) => (num(a)?, num(b)?),
        None => (num(s)?, num(s)?),
    };
    VersionRange::new(min, max).ok_or(super::AgentError::Parse("version range"))
}

/// A range raised to start at `floor`, as old links are adjusted on parsing.
#[must_use]
pub fn raise_range(vr: VersionRange, floor: Version) -> VersionRange {
    let min = floor.max(vr.min());
    VersionRange::new(min, min.max(vr.max())).unwrap_or(vr)
}
