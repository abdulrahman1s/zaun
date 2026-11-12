//! Executables of a game and what is known about them (DATA_MODEL §2, GAME_IDENTITY §3).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Problem, ValidationError};
use crate::clock::Timestamp;
use crate::ids::ExecutableId;

/// A SHA-256 digest.
pub type Sha256 = [u8; 32];

/// The role of an executable within its game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExeRole {
    /// The program Play starts. Exactly one per game.
    Primary,
    /// Another way to start the game (e.g. a DirectX 12 build).
    Alternate,
    /// A launcher shipped with the game.
    Launcher,
    /// A configuration tool or patcher.
    Tool,
    /// An installer.
    Installer,
}

impl ExeRole {
    /// Text form for the `executables.role` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            ExeRole::Primary => "primary",
            ExeRole::Alternate => "alternate",
            ExeRole::Launcher => "launcher",
            ExeRole::Tool => "tool",
            ExeRole::Installer => "installer",
        }
    }

    /// Parse [`ExeRole::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        [
            ExeRole::Primary,
            ExeRole::Alternate,
            ExeRole::Launcher,
            ExeRole::Tool,
            ExeRole::Installer,
        ]
        .into_iter()
        .find(|r| r.as_str() == s)
    }
}

/// How a file is recognised again (GAME_IDENTITY §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint {
    /// File size in bytes.
    pub size: u64,
    /// Modification time, nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
    /// SHA-256 over the size and the first and last `QUICK_HASH_WINDOW` bytes.
    #[serde(with = "hex32")]
    pub quick_hash: Sha256,
    /// SHA-256 of the whole file, when computed.
    #[serde(with = "hex32_opt")]
    pub full_hash: Option<Sha256>,
}

/// Machine type from the PE header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PeMachine {
    /// i386.
    #[serde(rename = "x86")]
    X86,
    /// AMD64.
    #[serde(rename = "x86_64")]
    X86_64,
    /// ARM64.
    #[serde(rename = "arm64")]
    Arm64,
    /// Anything else.
    #[serde(rename = "other")]
    Other,
    /// Not determined.
    #[default]
    #[serde(rename = "unknown")]
    Unknown,
}

/// Subsystem from the PE optional header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeSubsystem {
    /// Windows GUI.
    Gui,
    /// Console.
    Console,
    /// Anything else.
    Other,
    /// Not determined.
    #[default]
    Unknown,
}

/// Bounded, sanitised information read from a PE file. Every string is untrusted input that
/// `identity::pe` has length-capped and stripped of control characters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeInfo {
    /// `ProductName` version resource.
    pub product_name: Option<String>,
    /// `FileDescription`.
    pub file_description: Option<String>,
    /// `CompanyName`.
    pub company: Option<String>,
    /// `OriginalFilename`.
    pub original_filename: Option<String>,
    /// `FileVersion`.
    pub file_version: Option<String>,
    /// Machine type.
    pub machine: PeMachine,
    /// Subsystem.
    pub subsystem: PeSubsystem,
    /// Link timestamp.
    pub timestamp: Option<u32>,
    /// Has an icon group resource.
    pub has_icon: bool,
    /// Is a DLL.
    pub is_dll: bool,
    /// Has a CLR header.
    pub is_dotnet: bool,
    /// Parser notes (e.g. "resource section truncated").
    pub notes: Vec<String>,
}

/// One executable of a game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Executable {
    /// Identifier.
    pub id: ExecutableId,
    /// Role.
    pub role: ExeRole,
    /// The path as chosen.
    pub path: PathBuf,
    /// Canonical path (unique across the library).
    pub canonical_path: PathBuf,
    /// Relative to `Game.root`.
    pub rel_path: Option<PathBuf>,
    /// Mount point of the filesystem it is on (distinguishes "drive not connected").
    pub mount_point: Option<PathBuf>,
    /// Fingerprint.
    pub fingerprint: Fingerprint,
    /// PE information.
    pub pe: PeInfo,
    /// Arguments replacing the game's arguments for this executable.
    pub args_override: Option<Vec<String>>,
    /// Label, e.g. "DirectX 12", "Config tool".
    pub label: Option<String>,
    /// When it was added.
    pub added_at: Timestamp,
}

/// Maximum length of an executable label, in characters.
pub const EXE_LABEL_MAX_CHARS: usize = 40;

impl Executable {
    /// Paths absolute, label and argument limits.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !self.path.is_absolute() {
            return Err(ValidationError::new(
                "executable.path",
                Problem::NotAbsolute,
            ));
        }
        if !self.canonical_path.is_absolute() {
            return Err(ValidationError::new(
                "executable.canonical_path",
                Problem::NotAbsolute,
            ));
        }
        if let Some(rel) = &self.rel_path
            && (rel.is_absolute()
                || rel
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)))
        {
            return Err(ValidationError::new(
                "executable.rel_path",
                Problem::InvalidFormat,
            ));
        }
        if let Some(label) = &self.label {
            if label.trim().is_empty() {
                return Err(ValidationError::new("executable.label", Problem::Empty));
            }
            if label.chars().count() > EXE_LABEL_MAX_CHARS {
                return Err(ValidationError::new(
                    "executable.label",
                    Problem::TooLong {
                        max: EXE_LABEL_MAX_CHARS,
                    },
                ));
            }
        }
        if let Some(args) = &self.args_override {
            super::launch_config::validate_args("executable.args_override", args)?;
        }
        Ok(())
    }
}

/// Lower-case hexadecimal text of a digest.
pub fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(HEX[usize::from(b >> 4)]));
        s.push(char::from(HEX[usize::from(b & 0x0f)]));
    }
    s
}

/// Parse 64 hexadecimal digits.
pub fn from_hex32(s: &str) -> Option<Sha256> {
    let bytes = s.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let digit = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let mut out = [0u8; 32];
    for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        out[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(out)
}

/// Serde helper: a digest as 64 lower-case hexadecimal digits.
pub(crate) mod hex32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(v: &super::Sha256, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::to_hex(v))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<super::Sha256, D::Error> {
        let s = String::deserialize(d)?;
        super::from_hex32(&s).ok_or_else(|| serde::de::Error::custom("expected 64 hex digits"))
    }
}

/// Serde helper: an optional digest as hexadecimal text or `null`.
pub(crate) mod hex32_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(
        v: &Option<super::Sha256>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match v {
            Some(h) => s.serialize_some(&super::to_hex(h)),
            None => s.serialize_none(),
        }
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<super::Sha256>, D::Error> {
        let s = Option::<String>::deserialize(d)?;
        match s {
            None => Ok(None),
            Some(s) => super::from_hex32(&s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom("expected 64 hex digits")),
        }
    }
}
