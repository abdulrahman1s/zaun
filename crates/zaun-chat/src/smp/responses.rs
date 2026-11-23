//! Server responses and pushes with their versioned encodings (Protocol.hs `instance ProtocolEncoding BrokerMsg`, simplexmq 7.1.4).
use crate::crypto::PublicKey;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Tail};

use super::queue_data::{QueueLinkData, QueueMode};
use super::server_error::ErrorType;
use super::version::{NEW_NTF_CREDS, SERVICE_CERTS};

/// `ServerNtfCreds`: the notifier id and server DH key NEW returns at v17 and later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerNtfCreds {
    pub notifier_id: Vec<u8>,
    pub dh_key: PublicKey,
}

/// `QueueIdsKeys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueIds {
    pub rcv_id: Vec<u8>,
    pub snd_id: Vec<u8>,
    /// Server key for the server-to-recipient message layer.
    pub rcv_dh_key: PublicKey,
    pub queue_mode: Option<QueueMode>,
    pub link_id: Option<Vec<u8>>,
    pub service_id: Option<Vec<u8>>,
    pub ntf_creds: Option<ServerNtfCreds>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    Ids(QueueIds),
    Lnk {
        sender_id: Vec<u8>,
        data: QueueLinkData,
    },
    /// `body` is still under the server-to-recipient box; see `message::decrypt_message`.
    Msg {
        msg_id: Vec<u8>,
        body: Vec<u8>,
    },
    Ok,
    Sok(Option<Vec<u8>>),
    Nid {
        notifier_id: Vec<u8>,
        dh_key: PublicKey,
    },
    End,
    Deld,
    Info(Vec<u8>),
    Err(ErrorType),
    Pong,
}

impl Response {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Ids(_) => "IDS",
            Self::Lnk { .. } => "LNK",
            Self::Msg { .. } => "MSG",
            Self::Ok => "OK",
            Self::Sok(_) => "SOK",
            Self::Nid { .. } => "NID",
            Self::End => "END",
            Self::Deld => "DELD",
            Self::Info(_) => "INFO",
            Self::Err(_) => "ERR",
            Self::Pong => "PONG",
        }
    }

    /// `protocolP v` for `BrokerMsg`; the whole input must be used (MSG and INFO take the rest).
    pub fn decode(v: u16, bytes: &[u8]) -> Result<Self, DecodeError> {
        let (tag, rest) =
            bytes.split_at(bytes.iter().position(|b| *b == b' ').unwrap_or(bytes.len()));
        let params = rest.get(1..).unwrap_or_default();
        let mut d = Decoder::new(params);
        let r = match tag {
            b"IDS" => Self::Ids(decode_ids(v, &mut d)?),
            b"LNK" => Self::Lnk {
                sender_id: Vec::decode(&mut d)?,
                data: QueueLinkData::decode(&mut d)?,
            },
            b"MSG" => Self::Msg {
                msg_id: Vec::decode(&mut d)?,
                body: Tail::decode(&mut d)?.0,
            },
            b"SOK" => Self::Sok(Option::decode(&mut d)?),
            b"NID" => Self::Nid {
                notifier_id: Vec::decode(&mut d)?,
                dh_key: PublicKey::decode(&mut d)?,
            },
            b"INFO" => Self::Info(Tail::decode(&mut d)?.0),
            b"ERR" => Self::Err(ErrorType::parse(d.take_all())?),
            b"OK" => Self::Ok,
            b"END" => Self::End,
            b"DELD" => Self::Deld,
            b"PONG" => Self::Pong,
            _ => return Err(DecodeError::Invalid("response tag")),
        };
        d.finish()?;
        Ok(r)
    }

    /// `encodeProtocol v`: what a server writes. The client only needs it to drive tests.
    pub fn encode(&self, v: u16) -> Result<Vec<u8>, EncodeError> {
        let mut out = self.name().as_bytes().to_vec();
        out.push(b' ');
        match self {
            Self::Ids(q) => {
                q.rcv_id.encode(&mut out)?;
                q.snd_id.encode(&mut out)?;
                q.rcv_dh_key.encode(&mut out)?;
                q.queue_mode.encode(&mut out)?;
                q.link_id.encode(&mut out)?;
                if v >= SERVICE_CERTS {
                    q.service_id.encode(&mut out)?;
                }
                if v >= NEW_NTF_CREDS {
                    q.ntf_creds
                        .as_ref()
                        .map(|n| (&n.notifier_id, &n.dh_key))
                        .encode(&mut out)?;
                }
            }
            Self::Lnk { sender_id, data } => {
                sender_id.encode(&mut out)?;
                data.encode(&mut out)?;
            }
            Self::Msg { msg_id, body } => {
                msg_id.encode(&mut out)?;
                out.extend_from_slice(body);
            }
            Self::Sok(id) if v >= SERVICE_CERTS => id.encode(&mut out)?,
            Self::Sok(_) => return Self::Ok.encode(v),
            Self::Nid {
                notifier_id,
                dh_key,
            } => {
                notifier_id.encode(&mut out)?;
                dh_key.encode(&mut out)?;
            }
            Self::Info(b) => out.extend_from_slice(b),
            Self::Err(e) => out.extend_from_slice(e.to_string().as_bytes()),
            Self::Ok | Self::End | Self::Deld | Self::Pong => {
                out.pop();
            }
        }
        Ok(out)
    }
}

fn decode_ids(v: u16, d: &mut Decoder<'_>) -> Result<QueueIds, DecodeError> {
    Ok(QueueIds {
        rcv_id: Vec::decode(d)?,
        snd_id: Vec::decode(d)?,
        rcv_dh_key: PublicKey::decode(d)?,
        queue_mode: Option::decode(d)?,
        link_id: Option::decode(d)?,
        service_id: if v >= SERVICE_CERTS {
            Option::decode(d)?
        } else {
            None
        },
        ntf_creds: if v >= NEW_NTF_CREDS {
            Option::<(Vec<u8>, PublicKey)>::decode(d)?.map(|(notifier_id, dh_key)| ServerNtfCreds {
                notifier_id,
                dh_key,
            })
        } else {
            None
        },
    })
}
