//! Full connection links: `simplex:/invitation#/?v=6-8&smp=…&e2e=…` and `https://<host>/contact#/?…` (Agent/Protocol.hs `ConnectionRequestUri`, AGPL-3.0-only).
use crate::encoding::{b64_decode, b64_encode};

use super::binary_req::BinaryConnReq;
use super::conn_req_data::{ConnReqUriData, ServiceScheme};
use crate::agent::e2e::E2eParamsUri;
use crate::agent::error::{AgentError, Result};
use crate::agent::kem::KemParams;
use crate::agent::query::{self, pair};
use crate::agent::queue_uri::SmpQueueUri;
use crate::agent::version::{MIN_SUPPORTED_AGENT, parse_range, raise_range, range_str};

/// Keys of a contact address that takes ratchet-encrypted requests (agent v8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressRatchetKeys {
    pub ratchet_key_id: Vec<u8>,
    pub e2e: E2eParamsUri,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionRequestUri {
    Invitation {
        data: ConnReqUriData,
        e2e: E2eParamsUri,
    },
    Contact {
        data: ConnReqUriData,
        ratchet_keys: Option<AddressRatchetKeys>,
    },
}

impl ConnectionRequestUri {
    #[must_use]
    pub fn data(&self) -> &ConnReqUriData {
        match self {
            Self::Invitation { data, .. } | Self::Contact { data, .. } => data,
        }
    }

    /// `simplexConnReqUri`.
    #[must_use]
    pub fn into_simplex(mut self) -> Self {
        match &mut self {
            Self::Invitation { data, .. } | Self::Contact { data, .. } => {
                data.scheme = ServiceScheme::Simplex
            }
        }
        self
    }

    /// `binaryConnReq`: what goes into link data; a contact link's ratchet keys travel in the user data instead.
    #[must_use]
    pub fn binary(&self) -> BinaryConnReq {
        match self {
            Self::Invitation { data, e2e } => BinaryConnReq::Invitation {
                data: data.clone(),
                e2e: e2e.clone(),
            },
            Self::Contact { data, .. } => BinaryConnReq::Contact { data: data.clone() },
        }
    }

    /// `strEncode`.
    #[must_use]
    pub fn to_uri(&self) -> String {
        let (mode, data, e2e, rk) = match self {
            Self::Invitation { data, e2e } => ("invitation", data, Some(e2e), None),
            Self::Contact { data, ratchet_keys } => {
                let keys = ratchet_keys.as_ref();
                (
                    "contact",
                    data,
                    keys.map(|k| &k.e2e),
                    keys.map(|k| &k.ratchet_key_id),
                )
            }
        };
        let queues: Vec<String> = data.smp_queues.iter().map(SmpQueueUri::to_uri).collect();
        let mut q = vec![
            pair("v", range_str(&data.agent_v_range)),
            pair("smp", queues.join(";")),
        ];
        q.extend(e2e.map(|e| pair("e2e", e.to_query())));
        q.extend(rk.map(|k| pair("rk", b64_encode(k, true))));
        q.extend(data.client_data.as_ref().map(|d| pair("data", d.as_str())));
        format!("{}/{mode}#/?{}", data.scheme.to_uri(), query::render(&q))
    }

    /// `strP`. The scheme is kept as written, so that printing returns the same text; the Haskell parser replaces it with `simplex:`.
    pub fn parse(s: &str) -> Result<Self> {
        let (scheme, rest) = ServiceScheme::parse_prefix(s.trim())?;
        let rest = rest
            .strip_prefix('/')
            .ok_or(AgentError::Parse("link path"))?;
        let (invitation, rest) = match (
            rest.strip_prefix("invitation"),
            rest.strip_prefix("contact"),
        ) {
            (Some(r), _) => (true, r),
            (_, Some(r)) => (false, r),
            _ => return Err(AgentError::Parse("link mode")),
        };
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        let q = rest
            .strip_prefix("#/?")
            .ok_or(AgentError::Parse("link query"))?;
        let q = query::parse(q);
        let v = query::get(&q, "v").ok_or(AgentError::MissingParam("v"))?;
        let mut agent_v_range = parse_range(v)?;
        let smp = query::get(&q, "smp").ok_or(AgentError::MissingParam("smp"))?;
        let smp_queues = smp
            .split(';')
            .map(SmpQueueUri::parse)
            .collect::<Result<Vec<_>>>()?;
        let client_data = query::get(&q, "data").map(str::to_owned);
        let e2e = query::get(&q, "e2e").map(E2eParamsUri::parse).transpose()?;
        if invitation {
            let e2e = e2e.ok_or(AgentError::MissingParam("e2e"))?;
            if matches!(e2e.kem, Some(KemParams::Accepted { .. })) {
                return Err(AgentError::Parse("e2e params of an invitation"));
            }
            let data = ConnReqUriData {
                scheme,
                agent_v_range,
                smp_queues,
                client_data,
            };
            return Ok(Self::Invitation { data, e2e });
        }
        // Old contact links are raised to the lowest version the agent supports.
        agent_v_range = raise_range(agent_v_range, MIN_SUPPORTED_AGENT);
        let ratchet_keys = match (e2e, query::get(&q, "rk")) {
            (Some(e2e), Some(rk)) => Some(AddressRatchetKeys {
                ratchet_key_id: b64_decode(rk)?,
                e2e,
            }),
            _ => None,
        };
        let data = ConnReqUriData {
            scheme,
            agent_v_range,
            smp_queues,
            client_data,
        };
        Ok(Self::Contact { data, ratchet_keys })
    }
}
