//! Strongly typed identifiers.
//!
//! Layer 1. UUIDv7 newtypes are created only here, from a [`Clock`] so that the time part is
//! controlled by the caller (no hidden `SystemTime::now`). They serialise as lower-case
//! hyphenated text (DATA_MODEL §1). No identifier implements `Default`: IDs are never
//! invented implicitly.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::clock::Clock;
use crate::consts::{RUNTIME_NAME_MAX_BYTES, TAG_NAME_MAX_CHARS};

/// Why a textual identifier was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// Not a hyphenated UUID.
    #[error("not a hyphenated UUID: {0:?}")]
    InvalidUuid(String),
    /// A runtime name outside `[A-Za-z0-9._+-]{1,96}`.
    #[error("invalid runtime name: {0:?}")]
    InvalidRuntimeName(String),
    /// A tag name that is empty after trimming.
    #[error("tag name is empty")]
    EmptyTagName,
    /// A tag name longer than 32 characters.
    #[error("tag name longer than {max} characters")]
    TagNameTooLong {
        /// The limit.
        max: usize,
    },
    /// A tag name containing control characters.
    #[error("tag name contains control characters")]
    TagNameControlChars,
}

/// Create a UUIDv7 whose timestamp comes from `clock`.
fn new_v7(clock: &dyn Clock) -> Uuid {
    let ms = clock.now().as_millis().max(0);
    let secs = u64::try_from(ms / 1000).unwrap_or(0);
    let nanos = u32::try_from((ms % 1000) * 1_000_000).unwrap_or(0);
    Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, secs, nanos))
}

fn parse_hyphenated(s: &str) -> Result<Uuid, IdError> {
    if s.len() != 36 {
        return Err(IdError::InvalidUuid(s.to_owned()));
    }
    Uuid::try_parse(s).map_err(|_| IdError::InvalidUuid(s.to_owned()))
}

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// A new, time-ordered identifier (UUIDv7) using `clock` for its time part.
            pub fn generate(clock: &dyn Clock) -> Self {
                Self(new_v7(clock))
            }

            /// Wrap an existing UUID (storage, fixtures).
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// The UUID value (a copy; the inner value cannot be mutated).
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }

            /// The first eight hexadecimal digits, as used in log lines.
            pub fn short(&self) -> String {
                let mut s = self.0.simple().to_string();
                s.truncate(8);
                s
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.hyphenated(), f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0.hyphenated())
            }
        }

        impl FromStr for $name {
            type Err = IdError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                parse_hyphenated(s).map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut buf = Uuid::encode_buffer();
                s.serialize_str(self.0.hyphenated().encode_lower(&mut buf))
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

uuid_id!(
    /// A game in the library.
    GameId
);
/// The fixed upper 96 bits of a Steam game's [`GameId`] (ADR-0031): `"STEAM"` in ASCII, the
/// UUID version nibble 8 (custom) and the RFC 4122 variant. Generated IDs are version 7, so
/// the two kinds can never collide.
const STEAM_ID_PREFIX: u128 = 0x5354_4541_4d00_8000_8000_0000_0000_0000;
const STEAM_ID_MASK: u128 = !0xFFFF_FFFF;

impl GameId {
    /// The identifier of the Steam game with app ID `app_id` (ADR-0031): the app ID is the
    /// game's identity; this is its spelling in EasyPlay's vocabulary, the same on every
    /// machine and every scan. `app_id` 0 is not a Steam app; callers never pass it.
    pub const fn steam(app_id: u32) -> Self {
        Self(Uuid::from_u128(STEAM_ID_PREFIX | app_id as u128))
    }

    /// The Steam app ID when this identifies a Steam game.
    pub const fn steam_app_id(&self) -> Option<u32> {
        let v = self.0.as_u128();
        if v & STEAM_ID_MASK == STEAM_ID_PREFIX && v as u32 != 0 {
            Some(v as u32)
        } else {
            None
        }
    }

    /// Whether this identifies a Steam game.
    pub const fn is_steam(&self) -> bool {
        self.steam_app_id().is_some()
    }
}

uuid_id!(
    /// An executable belonging to a game.
    ExecutableId
);
uuid_id!(
    /// One sandbox lifetime (ADR-0013).
    SessionId
);
uuid_id!(
    /// One launch attempt as seen by the UI, before and after a session exists.
    LaunchId
);
uuid_id!(
    /// A prompt raised by a parked flow.
    PromptId
);
uuid_id!(
    /// A query whose result arrives as `Event::QueryResult`.
    QueryId
);
uuid_id!(
    /// A background job.
    JobId
);

/// A tag row identifier (`tags.id`, an SQLite integer key).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TagId(i64);

impl TagId {
    /// Wrap a database key.
    pub const fn new(raw: i64) -> Self {
        Self(raw)
    }
    /// The database key.
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for TagId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// The name of a Proton build in the runtime store, e.g. `GE-Proton10-25`.
/// Validated: `[A-Za-z0-9._+-]{1,96}`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RuntimeName(String);

impl RuntimeName {
    /// Validate and wrap a runtime name.
    pub fn new(name: impl Into<String>) -> Result<Self, IdError> {
        let name = name.into();
        let valid = !name.is_empty()
            && name.len() <= RUNTIME_NAME_MAX_BYTES
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'));
        if valid {
            Ok(Self(name))
        } else {
            Err(IdError::InvalidRuntimeName(name))
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuntimeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RuntimeName {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for RuntimeName {
    type Error = IdError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<RuntimeName> for String {
    fn from(n: RuntimeName) -> Self {
        n.0
    }
}

/// A tag name: trimmed, 1–32 characters, case-preserving, compared case-insensitively.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TagName(String);

impl TagName {
    /// Trim, validate and wrap a tag name.
    pub fn new(name: impl AsRef<str>) -> Result<Self, IdError> {
        let trimmed = name.as_ref().trim();
        if trimmed.is_empty() {
            return Err(IdError::EmptyTagName);
        }
        if trimmed.chars().count() > TAG_NAME_MAX_CHARS {
            return Err(IdError::TagNameTooLong {
                max: TAG_NAME_MAX_CHARS,
            });
        }
        if trimmed.chars().any(char::is_control) {
            return Err(IdError::TagNameControlChars);
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The name as entered (trimmed).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The comparison key (lower case).
    pub fn key(&self) -> String {
        self.0.to_lowercase()
    }
}

impl PartialEq for TagName {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl Eq for TagName {}

impl std::hash::Hash for TagName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

impl PartialOrd for TagName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TagName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key()
            .cmp(&other.key())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl fmt::Display for TagName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TagName {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for TagName {
    type Error = IdError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<TagName> for String {
    fn from(n: TagName) -> Self {
        n.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{MonoInstant, Timestamp};

    struct At(i64);
    impl Clock for At {
        fn now(&self) -> Timestamp {
            Timestamp::from_millis(self.0)
        }
        fn mono(&self) -> MonoInstant {
            MonoInstant::ZERO
        }
    }

    #[test]
    fn steam_ids_round_trip_and_never_match_generated_ones() {
        for app in [1, 10, 1_174_180, u32::MAX] {
            let id = GameId::steam(app);
            assert_eq!(id.steam_app_id(), Some(app));
            assert_eq!(id.as_uuid().get_version_num(), 8);
            assert_eq!(id.to_string().parse::<GameId>().unwrap(), id);
        }
        assert_eq!(GameId::steam(0).steam_app_id(), None);
        let generated = GameId::generate(&At(1_794_480_000_123));
        assert!(!generated.is_steam());
        assert_eq!(
            GameId::steam(440).to_string(),
            "53544541-4d00-8000-8000-0000000001b8"
        );
    }

    #[test]
    fn generated_ids_are_v7_and_carry_the_clock_time() {
        let id = GameId::generate(&At(1_794_480_000_123));
        let uuid = id.as_uuid();
        assert_eq!(uuid.get_version_num(), 7);
        let (secs, nanos) = uuid.get_timestamp().unwrap().to_unix();
        assert_eq!(secs, 1_794_480_000);
        assert_eq!(nanos / 1_000_000, 123);
    }

    #[test]
    fn generated_ids_sort_by_time() {
        let a = SessionId::generate(&At(1_000));
        let b = SessionId::generate(&At(2_000));
        assert!(a < b);
    }

    #[test]
    fn id_text_round_trip_and_serde() {
        let text = "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e";
        let id: GameId = text.parse().unwrap();
        assert_eq!(id.to_string(), text);
        assert_eq!(id.short(), "0192f0c2");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        let back: GameId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
        // Upper case accepted on input, written in lower case.
        let upper: GameId = text.to_uppercase().parse().unwrap();
        assert_eq!(upper.to_string(), text);
    }

    #[test]
    fn id_parsing_rejects_other_forms() {
        for bad in [
            "",
            "0192f0c27b1e7c3a9d2f5e1a2b3c4d5e",
            "{0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e}",
            "urn:uuid:0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e",
            "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5z",
        ] {
            assert!(bad.parse::<GameId>().is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<ExecutableId>("\"nope\"").is_err());
    }

    #[test]
    fn runtime_name_validation_table() {
        let cases: &[(&str, bool)] = &[
            ("GE-Proton10-25", true),
            ("UMU-Proton-10.0-4", true),
            ("proton_9.0+beta", true),
            ("a", true),
            ("", false),
            ("GE Proton", false),
            ("../etc", false),
            ("name/with/slash", false),
            ("ümlaut", false),
        ];
        for (name, ok) in cases {
            assert_eq!(RuntimeName::new(*name).is_ok(), *ok, "{name:?}");
        }
        assert!(RuntimeName::new("x".repeat(96)).is_ok());
        assert!(RuntimeName::new("x".repeat(97)).is_err());
        let n: RuntimeName = serde_json::from_str("\"GE-Proton10-25\"").unwrap();
        assert_eq!(serde_json::to_string(&n).unwrap(), "\"GE-Proton10-25\"");
        assert!(serde_json::from_str::<RuntimeName>("\"bad name\"").is_err());
    }

    #[test]
    fn tag_name_validation_and_comparison() {
        let t = TagName::new("  Co-op  ").unwrap();
        assert_eq!(t.as_str(), "Co-op");
        assert_eq!(t, TagName::new("CO-OP").unwrap());
        assert_eq!(TagName::new("   "), Err(IdError::EmptyTagName));
        assert!(TagName::new("é".repeat(32)).is_ok());
        assert_eq!(
            TagName::new("x".repeat(33)),
            Err(IdError::TagNameTooLong { max: 32 })
        );
        assert_eq!(TagName::new("a\tb"), Err(IdError::TagNameControlChars));
        let mut set = std::collections::HashSet::new();
        set.insert(TagName::new("RPG").unwrap());
        assert!(set.contains(&TagName::new("rpg").unwrap()));
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, "\"Co-op\"");
        assert!(serde_json::from_str::<TagName>("\"\"").is_err());
    }

    #[test]
    fn tag_id_serde() {
        let id = TagId::new(42);
        assert_eq!(serde_json::to_string(&id).unwrap(), "42");
        assert_eq!(id.get(), 42);
    }
}
