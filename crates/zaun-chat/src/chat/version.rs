//! Chat and call version ranges and the gates of 7.1.4 (Protocol.hs, Call.hs).
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionRange {
    pub min: u16,
    pub max: u16,
}

pub const INITIAL_CHAT_VERSION: u16 = 9;
pub const CURRENT_CHAT_VERSION: u16 = 22;
pub const CHAT_INITIAL_VRANGE: VersionRange = VersionRange::single(INITIAL_CHAT_VERSION);
pub const SUPPORTED_CHAT_VRANGE: VersionRange =
    VersionRange::new(INITIAL_CHAT_VERSION, CURRENT_CHAT_VERSION);

pub const BUSINESS_CHATS: u16 = 10;
pub const BUSINESS_CHAT_PREFS: u16 = 11;
pub const CONTENT_REPORTS: u16 = 12;
pub const GROUP_JOIN_REJECT: u16 = 14;
pub const GROUP_KNOCKING: u16 = 15;
pub const SHORT_LINK_DATA: u16 = 16;
pub const MEMBER_SUPPORT_VOICE: u16 = 17;
/// Also the version from which classic groups use the binary `=` batch and sign admin events.
pub const RELAY_WEB_CAP: u16 = 18;
pub const GROUP_ROSTER: u16 = 19;
pub const GROUP_MEMBER_KEY: u16 = 20;
pub const ANY_TEXT_COMMANDS: u16 = 21;
pub const GROUP_INV: u16 = 22;
pub const GROUP_BINARY_BATCH: u16 = RELAY_WEB_CAP;

pub const CALL_INITIAL_VERSION: u16 = 1;
/// Call media key derived with HKDF-SHA512 instead of the raw DH secret.
pub const CALL_MEDIA_KDF_VERSION: u16 = 2;
pub const SUPPORTED_CALL_VRANGE: VersionRange =
    VersionRange::new(CALL_INITIAL_VERSION, CALL_MEDIA_KDF_VERSION);

impl VersionRange {
    pub const fn new(min: u16, max: u16) -> Self {
        Self { min, max }
    }

    pub const fn single(v: u16) -> Self {
        Self { min: v, max: v }
    }

    pub const fn is_valid(&self) -> bool {
        self.min <= self.max
    }

    /// `compatibleVersion`: the highest version both sides speak.
    pub fn compatible(&self, other: &Self) -> Option<u16> {
        let (min, max) = (self.min.max(other.min), self.max.min(other.max));
        (min <= max).then_some(max)
    }

    /// Whether a feature gated at `version` is on for a peer whose maximum is this range's.
    pub const fn supports(&self, version: u16) -> bool {
        self.max >= version
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (a, b) = match s.split_once('-') {
            Some((a, b)) => (a, b),
            None => (s, s),
        };
        let r = Self::new(a.parse().ok()?, b.parse().ok()?);
        r.is_valid().then_some(r)
    }
}

impl std::fmt::Display for VersionRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.min == self.max {
            write!(f, "{}", self.min)
        } else {
            write!(f, "{}-{}", self.min, self.max)
        }
    }
}

impl Serialize for VersionRange {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for VersionRange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&s).ok_or_else(|| serde::de::Error::custom("bad version range"))
    }
}
