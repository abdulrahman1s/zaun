//! A queue address with one agreed client version, as sent inside messages (Agent/Protocol.hs `SMPQueueInfo`, AGPL-3.0-only).
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Version, VersionRange};

use super::error::Result;
use super::queue_mode::sender_can_secure;
use super::queue_uri::{SmpQueueAddress, SmpQueueUri};
use super::version::{INITIAL_CLIENT, SHORT_LINKS_CLIENT, SND_AUTH_KEY_CLIENT};
use crate::smp::SmpServer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmpQueueInfo {
    pub client_version: Version,
    pub address: SmpQueueAddress,
}

impl SmpQueueInfo {
    /// `toVersionT`: the queue of a link at the version both sides agreed on.
    #[must_use]
    pub fn from_uri(uri: &SmpQueueUri, version: Version) -> Self {
        Self {
            client_version: version,
            address: uri.address.clone(),
        }
    }

    /// `strEncode`: the queue URI of the one-version range.
    #[must_use]
    pub fn to_uri(&self) -> String {
        self.to_queue_uri().to_uri()
    }

    #[must_use]
    pub fn to_queue_uri(&self) -> SmpQueueUri {
        SmpQueueUri {
            client_v_range: VersionRange::single(self.client_version),
            address: self.address.clone(),
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let uri = SmpQueueUri::parse(s)?;
        Ok(Self {
            client_version: uri.client_v_range.max(),
            address: uri.address,
        })
    }
}

impl Encode for SmpQueueInfo {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let (v, a) = (self.client_version, &self.address);
        let secure = sender_can_secure(a.queue_mode);
        if v <= INITIAL_CLIENT {
            v.encode(out)?;
            a.server.encode_legacy(out)?;
            return (&a.sender_id, &a.dh_public_key).encode(out);
        }
        (v, &a.server, &a.sender_id, &a.dh_public_key).encode(out)?;
        if v >= SHORT_LINKS_CLIENT {
            a.queue_mode.map_or(Ok(()), |m| m.encode(out))
        } else if v >= SND_AUTH_KEY_CLIENT && secure {
            secure.encode(out)
        } else {
            Ok(())
        }
    }
}

impl Decode for SmpQueueInfo {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let client_version = Version::decode(d)?;
        let server = if client_version > INITIAL_CLIENT {
            SmpServer::decode(d)?
        } else {
            SmpServer::decode_legacy(d)?
        };
        let (sender_id, dh_public_key, queue_mode) = SmpQueueAddress::decode_tail(d)?;
        let address = SmpQueueAddress {
            server,
            sender_id,
            dh_public_key,
            queue_mode,
        };
        Ok(Self {
            client_version,
            address,
        })
    }
}
