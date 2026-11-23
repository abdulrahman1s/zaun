//! The common part of invitation and contact links: scheme, agent versions, queues, client data (Agent/Protocol.hs `ConnReqUriData`, `ServiceScheme`, AGPL-3.0-only).
use crate::encoding::{
    Decode, DecodeError, Decoder, Encode, EncodeError, Large, NonEmpty, VersionRange,
};

use crate::agent::error::{AgentError, Result};
use crate::agent::queue_mode::QueueMode;
use crate::agent::queue_uri::SmpQueueUri;

/// Where a link claims to be served from: the `simplex:` scheme or `https://<host>[:port]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceScheme {
    Simplex,
    AppServer(String),
}

impl ServiceScheme {
    #[must_use]
    pub fn to_uri(&self) -> String {
        match self {
            Self::Simplex => "simplex:".to_owned(),
            Self::AppServer(host) => format!("https://{host}"),
        }
    }

    /// The scheme at the start of `s`, and the rest from the `/` after the host.
    pub fn parse_prefix(s: &str) -> Result<(Self, &str)> {
        if let Some(rest) = s.strip_prefix("simplex:") {
            return Ok((Self::Simplex, rest));
        }
        let rest = s
            .strip_prefix("https://")
            .ok_or(AgentError::Parse("link scheme"))?;
        let end = rest
            .find('/')
            .ok_or(AgentError::Parse("link without path"))?;
        let host = &rest[..end];
        if host.is_empty()
            || host.contains(['#', ',', ';', '?'])
            || host.contains(char::is_whitespace)
        {
            return Err(AgentError::Parse("link host"));
        }
        Ok((Self::AppServer(host.to_owned()), &rest[end..]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnReqUriData {
    pub scheme: ServiceScheme,
    pub agent_v_range: VersionRange,
    /// Never empty.
    pub smp_queues: Vec<SmpQueueUri>,
    pub client_data: Option<String>,
}

/// The binary form has no scheme: it is `simplex:` on decoding.
impl Encode for ConnReqUriData {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let queues = NonEmpty::new(self.smp_queues.clone()).ok_or(EncodeError::EmptyList)?;
        let data = self
            .client_data
            .as_ref()
            .map(|s| Large(s.as_bytes().to_vec()));
        (self.agent_v_range, &queues, &data).encode(out)
    }
}

impl Decode for ConnReqUriData {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (agent_v_range, queues, data) =
            <(VersionRange, NonEmpty<SmpQueueUri>, Option<Large>)>::decode(d)?;
        // A contact queue's mode is not in the encoding of old client versions.
        let smp_queues = queues
            .into_vec()
            .into_iter()
            .map(|mut q| {
                q.address.queue_mode.get_or_insert(QueueMode::Contact);
                q
            })
            .collect();
        let client_data = data
            .map(|l| String::from_utf8(l.0).map_err(|_| DecodeError::Utf8))
            .transpose()?;
        Ok(Self {
            scheme: ServiceScheme::Simplex,
            agent_v_range,
            smp_queues,
            client_data,
        })
    }
}
