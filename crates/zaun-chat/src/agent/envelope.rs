//! What goes into one SMP message body before per-queue encryption (Agent/Protocol.hs `AgentMsgEnvelope`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large, Tail, Version};

use crate::agent::e2e::E2eParams;
use crate::agent::links::ConnectionRequestUri;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentMsgEnvelope {
    /// `C`: a sender-secured or legacy confirmation; the first message of a queue.
    Confirmation {
        agent_version: Version,
        e2e_encryption: Option<E2eParams>,
        enc_conn_info: Vec<u8>,
    },
    /// `M`: a ratchet-encrypted agent message.
    Message {
        agent_version: Version,
        enc_agent_message: Vec<u8>,
    },
    /// `I`: a join request to a contact address; the info is encrypted per queue only.
    Invitation {
        agent_version: Version,
        conn_req: ConnectionRequestUri,
        conn_info: Vec<u8>,
    },
    /// `A`: a ratchet-encrypted join request to an address that publishes ratchet keys (agent v8).
    ContactRequest {
        agent_version: Version,
        e2e_snd_params: E2eParams,
        ratchet_key_id: Vec<u8>,
        enc_conn_info: Vec<u8>,
    },
    /// `R`: new ratchet keys, to re-synchronise; encrypted per queue only.
    RatchetKey {
        agent_version: Version,
        e2e_encryption: E2eParams,
        info: Vec<u8>,
    },
}

impl AgentMsgEnvelope {
    #[must_use]
    pub fn agent_version(&self) -> Version {
        match self {
            Self::Confirmation { agent_version, .. }
            | Self::Message { agent_version, .. }
            | Self::Invitation { agent_version, .. }
            | Self::ContactRequest { agent_version, .. }
            | Self::RatchetKey { agent_version, .. } => *agent_version,
        }
    }
}

impl Encode for AgentMsgEnvelope {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        match self {
            Self::Confirmation {
                agent_version,
                e2e_encryption,
                enc_conn_info,
            } => (
                agent_version,
                b'C',
                e2e_encryption,
                Tail(enc_conn_info.clone()),
            )
                .encode(out),
            Self::Message {
                agent_version,
                enc_agent_message,
            } => (agent_version, b'M', Tail(enc_agent_message.clone())).encode(out),
            Self::Invitation {
                agent_version,
                conn_req,
                conn_info,
            } => {
                let req = Large(conn_req.to_uri().into_bytes());
                (agent_version, b'I', req, Tail(conn_info.clone())).encode(out)
            }
            Self::ContactRequest {
                agent_version,
                e2e_snd_params,
                ratchet_key_id,
                enc_conn_info,
            } => (
                agent_version,
                b'A',
                e2e_snd_params,
                ratchet_key_id,
                Tail(enc_conn_info.clone()),
            )
                .encode(out),
            Self::RatchetKey {
                agent_version,
                e2e_encryption,
                info,
            } => (agent_version, b'R', e2e_encryption, Tail(info.clone())).encode(out),
        }
    }
}

impl Decode for AgentMsgEnvelope {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let agent_version = Version::decode(d)?;
        match d.byte()? {
            b'C' => {
                // `SndE2ERatchetParams` takes any KEM state: a joiner proposes one to a link without it.
                let (e2e_encryption, Tail(enc_conn_info)) = <(Option<E2eParams>, Tail)>::decode(d)?;
                Ok(Self::Confirmation {
                    agent_version,
                    e2e_encryption,
                    enc_conn_info,
                })
            }
            b'M' => Ok(Self::Message {
                agent_version,
                enc_agent_message: Tail::decode(d)?.0,
            }),
            b'I' => {
                let (Large(req), Tail(conn_info)) = <(Large, Tail)>::decode(d)?;
                let conn_req = std::str::from_utf8(&req)
                    .map_err(|_| DecodeError::Utf8)
                    .and_then(|s| {
                        ConnectionRequestUri::parse(s)
                            .map_err(|_| DecodeError::Invalid("connection request"))
                    })?;
                match conn_req {
                    ConnectionRequestUri::Invitation { .. } => Ok(Self::Invitation {
                        agent_version,
                        conn_req,
                        conn_info,
                    }),
                    ConnectionRequestUri::Contact { .. } => {
                        Err(DecodeError::Invalid("connection request mode"))
                    }
                }
            }
            b'A' => {
                let (e2e_snd_params, ratchet_key_id, Tail(enc_conn_info)) =
                    <(E2eParams, Vec<u8>, Tail)>::decode(d)?;
                Ok(Self::ContactRequest {
                    agent_version,
                    e2e_snd_params,
                    ratchet_key_id,
                    enc_conn_info,
                })
            }
            b'R' => {
                let (params, Tail(info)) = <(E2eParams, Tail)>::decode(d)?;
                Ok(Self::RatchetKey {
                    agent_version,
                    e2e_encryption: params.check_proposed()?,
                    info,
                })
            }
            t => Err(DecodeError::BadTag("AgentMsgEnvelope", t)),
        }
    }
}
