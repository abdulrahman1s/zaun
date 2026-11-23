//! The server's answers (Protocol.hs `FileResponse`, `protocolP`; nook xftp/commands.ts `decodeResponse`).
use crate::crypto::{CbNonce, PublicKey};
use crate::encoding::{Decode, DecodeError, Decoder, NonEmpty};

use super::error::XftpError;
use super::error_type::XftpErrorType;
use super::version::FILE_STORAGE_TIME;

/// The sender id, the recipient ids and the expiry of a `SIDS` answer.
pub(super) type SndIds = (Vec<u8>, Vec<Vec<u8>>, Option<i64>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// A new chunk: its sender id, the recipient ids (one per key) and, at v4, when it expires (epoch seconds).
    SndIds {
        sender_id: Vec<u8>,
        recipient_ids: Vec<Vec<u8>>,
        expires: Option<i64>,
    },
    RcvIds {
        recipient_ids: Vec<Vec<u8>>,
    },
    /// The server's DH key and the nonce for the chunk that follows the response block.
    File {
        dh_key: PublicKey,
        nonce: CbNonce,
    },
    Ok,
    Err(XftpErrorType),
    Pong,
}

fn ids(d: &mut Decoder<'_>) -> Result<Vec<Vec<u8>>, DecodeError> {
    Ok(NonEmpty::<Vec<u8>>::decode(d)?.into_vec())
}

/// `GSTExpires`: `'T'` and the epoch seconds.
fn granted(d: &mut Decoder<'_>) -> Result<i64, DecodeError> {
    match d.byte()? {
        b'T' => i64::decode(d),
        t => Err(DecodeError::BadTag("GrantedStorageTime", t)),
    }
}

impl Response {
    /// Trailing bytes are ignored, as newer servers may append fields.
    pub fn decode(version: u16, bytes: &[u8]) -> Result<Self, XftpError> {
        let end = bytes.iter().position(|b| *b == b' ').unwrap_or(bytes.len());
        let (tag, rest) = (&bytes[..end], bytes.get(end + 1..).unwrap_or_default());
        let mut d = Decoder::new(rest);
        Ok(match tag {
            b"SIDS" => {
                let sender_id = Vec::<u8>::decode(&mut d)?;
                let recipient_ids = ids(&mut d)?;
                let expires = if version >= FILE_STORAGE_TIME {
                    optional_granted(&mut d)?
                } else {
                    None
                };
                Self::SndIds {
                    sender_id,
                    recipient_ids,
                    expires,
                }
            }
            b"RIDS" => Self::RcvIds {
                recipient_ids: ids(&mut d)?,
            },
            b"FILE" => Self::File {
                dh_key: PublicKey::decode(&mut d)?,
                nonce: CbNonce::new(d.take_array()?),
            },
            b"OK" => Self::Ok,
            b"PONG" => Self::Pong,
            b"ERR" => {
                Self::Err(XftpErrorType::parse(rest).ok_or(XftpError::Unexpected("error type"))?)
            }
            _ => return Err(XftpError::Unexpected("response tag")),
        })
    }
}

fn optional_granted(d: &mut Decoder<'_>) -> Result<Option<i64>, DecodeError> {
    match d.byte()? {
        b'0' => Ok(None),
        b'1' => granted(d).map(Some),
        t => Err(DecodeError::BadTag("Maybe", t)),
    }
}

/// What a command expects back; each accessor lists every variant, so a new response is a compile error here.
impl Response {
    pub(super) fn into_snd_ids(self) -> Option<SndIds> {
        match self {
            Self::SndIds {
                sender_id,
                recipient_ids,
                expires,
            } => Some((sender_id, recipient_ids, expires)),
            Self::RcvIds { .. } | Self::File { .. } | Self::Ok | Self::Err(_) | Self::Pong => None,
        }
    }

    pub(super) fn into_rcv_ids(self) -> Option<Vec<Vec<u8>>> {
        match self {
            Self::RcvIds { recipient_ids } => Some(recipient_ids),
            Self::SndIds { .. } | Self::File { .. } | Self::Ok | Self::Err(_) | Self::Pong => None,
        }
    }

    pub(super) fn into_file(self) -> Option<(PublicKey, CbNonce)> {
        match self {
            Self::File { dh_key, nonce } => Some((dh_key, nonce)),
            Self::SndIds { .. } | Self::RcvIds { .. } | Self::Ok | Self::Err(_) | Self::Pong => {
                None
            }
        }
    }

    pub(super) fn is_ok(&self) -> bool {
        match self {
            Self::Ok => true,
            Self::SndIds { .. }
            | Self::RcvIds { .. }
            | Self::File { .. }
            | Self::Err(_)
            | Self::Pong => false,
        }
    }

    pub(super) fn is_pong(&self) -> bool {
        match self {
            Self::Pong => true,
            Self::SndIds { .. }
            | Self::RcvIds { .. }
            | Self::File { .. }
            | Self::Ok
            | Self::Err(_) => false,
        }
    }
}
