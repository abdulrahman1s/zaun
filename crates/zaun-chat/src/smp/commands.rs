//! Client commands and their versioned encodings (Protocol.hs `instance ProtocolEncoding (Command p)`, simplexmq 7.1.4).
use crate::crypto::PublicKey;
use crate::encoding::{Encode, EncodeError};

use super::queue_data::{NewNtfCreds, QueueLinkData, QueueReqData, SubscriptionMode};
use super::version::NEW_NTF_CREDS;

/// `NewQueueReq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewQueue {
    pub rcv_auth_key: PublicKey,
    pub rcv_dh_key: PublicKey,
    pub basic_auth: Option<String>,
    pub sub_mode: SubscriptionMode,
    pub queue_req_data: Option<QueueReqData>,
    /// Only written at v17 and later.
    pub ntf_creds: Option<NewNtfCreds>,
}

impl NewQueue {
    /// What the 7.1.4 agent sends: a messaging queue (`QRMessaging Nothing`), no notifier.
    #[must_use]
    pub fn messaging(
        rcv_auth_key: PublicKey,
        rcv_dh_key: PublicKey,
        sub_mode: SubscriptionMode,
    ) -> Self {
        Self {
            rcv_auth_key,
            rcv_dh_key,
            basic_auth: None,
            sub_mode,
            queue_req_data: Some(QueueReqData::Messaging(None)),
            ntf_creds: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    New(Box<NewQueue>),
    Sub,
    Key(PublicKey),
    Skey(PublicKey),
    Lset {
        link_id: Vec<u8>,
        data: QueueLinkData,
    },
    Ldel,
    Lkey(PublicKey),
    Lget,
    Get,
    Ack {
        msg_id: Vec<u8>,
    },
    Off,
    Del,
    Que,
    Send {
        notification: bool,
        body: Vec<u8>,
    },
    Ping,
}

impl Command {
    /// The wire tag, also used in logs.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::New(_) => "NEW",
            Self::Sub => "SUB",
            Self::Key(_) => "KEY",
            Self::Skey(_) => "SKEY",
            Self::Lset { .. } => "LSET",
            Self::Ldel => "LDEL",
            Self::Lkey(_) => "LKEY",
            Self::Lget => "LGET",
            Self::Get => "GET",
            Self::Ack { .. } => "ACK",
            Self::Off => "OFF",
            Self::Del => "DEL",
            Self::Que => "QUE",
            Self::Send { .. } => "SEND",
            Self::Ping => "PING",
        }
    }

    /// `encodeProtocol v cmd`.
    pub fn encode(&self, v: u16) -> Result<Vec<u8>, EncodeError> {
        let mut out = self.name().as_bytes().to_vec();
        match self {
            Self::New(q) => {
                out.push(b' ');
                q.rcv_auth_key.encode(&mut out)?;
                q.rcv_dh_key.encode(&mut out)?;
                q.basic_auth.encode(&mut out)?;
                q.sub_mode.encode(&mut out)?;
                q.queue_req_data.encode(&mut out)?;
                if v >= NEW_NTF_CREDS {
                    q.ntf_creds.encode(&mut out)?;
                }
            }
            Self::Key(k) | Self::Skey(k) | Self::Lkey(k) => {
                out.push(b' ');
                k.encode(&mut out)?;
            }
            Self::Lset { link_id, data } => {
                out.push(b' ');
                link_id.encode(&mut out)?;
                data.encode(&mut out)?;
            }
            Self::Ack { msg_id } => {
                out.push(b' ');
                msg_id.encode(&mut out)?;
            }
            Self::Send { notification, body } => {
                out.push(b' ');
                notification.encode(&mut out)?;
                out.push(b' ');
                out.extend_from_slice(body);
            }
            Self::Sub
            | Self::Ldel
            | Self::Lget
            | Self::Get
            | Self::Off
            | Self::Del
            | Self::Que
            | Self::Ping => {}
        }
        Ok(out)
    }
}
