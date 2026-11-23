//! The XFTP commands and their encodings, gated on the negotiated version (Protocol.hs `FileCommand`, `encodeProtocol`).
use crate::crypto::PublicKey;
use crate::encoding::{Encode, EncodeError, NonEmpty};

use super::version::FILE_STORAGE_TIME;

/// What the server is told about a chunk: its sender key, size and SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub snd_key: PublicKey,
    pub size: u32,
    pub digest: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A new chunk with its first recipients; `storage_hours` is asked for at v4 and later only.
    New {
        info: FileInfo,
        rcv_keys: Vec<PublicKey>,
        basic_auth: Option<String>,
        storage_hours: Option<u32>,
    },
    AddRecipients {
        rcv_keys: Vec<PublicKey>,
    },
    Put,
    Delete,
    /// The X25519 key the chunk's transport encryption is agreed with.
    Get {
        rcv_dh_key: PublicKey,
    },
    Ack,
    Ping,
}

fn keys(rcv_keys: &[PublicKey]) -> Result<NonEmpty<&PublicKey>, EncodeError> {
    NonEmpty::new(rcv_keys.iter().collect()).ok_or(EncodeError::EmptyList)
}

impl Command {
    /// `encodeProtocol v`.
    pub fn encode(&self, version: u16) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        match self {
            Self::New {
                info,
                rcv_keys,
                basic_auth,
                storage_hours,
            } => {
                out.extend_from_slice(b"FNEW ");
                info.snd_key.encode(&mut out)?;
                info.size.encode(&mut out)?;
                info.digest.encode(&mut out)?;
                keys(rcv_keys)?.encode(&mut out)?;
                basic_auth.encode(&mut out)?;
                if version >= FILE_STORAGE_TIME {
                    storage_hours.encode(&mut out)?;
                }
            }
            Self::AddRecipients { rcv_keys } => {
                out.extend_from_slice(b"FADD ");
                keys(rcv_keys)?.encode(&mut out)?;
            }
            Self::Put => out.extend_from_slice(b"FPUT"),
            Self::Delete => out.extend_from_slice(b"FDEL"),
            Self::Get { rcv_dh_key } => {
                out.extend_from_slice(b"FGET ");
                rcv_dh_key.encode(&mut out)?;
            }
            Self::Ack => out.extend_from_slice(b"FACK"),
            Self::Ping => out.extend_from_slice(b"PING"),
        }
        Ok(out)
    }

    /// Whether the command is signed with the chunk's key; `PING` is not.
    #[must_use]
    pub fn is_authorized(&self) -> bool {
        !matches!(self, Self::Ping)
    }
}
