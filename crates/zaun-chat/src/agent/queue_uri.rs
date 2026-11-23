//! A queue address as a link component: `smp://…/<senderId>#/?v=1-4&dh=…&q=m` and its binary form (Agent/Protocol.hs `SMPQueueUri`, AGPL-3.0-only).
use crate::crypto::{Alg, PublicKey};
use crate::encoding::{
    Decode, DecodeError, Decoder, Encode, EncodeError, VersionRange, b64_encode,
};

use super::error::Result;
use super::keys::{expect_alg, key_str};
use super::query::{self, pair};
use super::queue_mode::{self, QueueMode, sender_can_secure};
use super::version::{SHORT_LINKS_CLIENT, SND_AUTH_KEY_CLIENT, SRV_HOSTNAMES_CLIENT, range_str};
use crate::smp::SmpServer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmpQueueAddress {
    pub server: SmpServer,
    pub sender_id: Vec<u8>,
    /// X25519.
    pub dh_public_key: PublicKey,
    pub queue_mode: Option<QueueMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmpQueueUri {
    pub client_v_range: VersionRange,
    pub address: SmpQueueAddress,
}

impl SmpQueueAddress {
    /// The part both queue forms write: version, server, sender id and DH key.
    pub(super) fn decode_tail(
        d: &mut Decoder<'_>,
    ) -> Result<(Vec<u8>, PublicKey, Option<QueueMode>), DecodeError> {
        let (sender_id, dh) = <(Vec<u8>, PublicKey)>::decode(d)?;
        Ok((
            sender_id,
            expect_alg(dh, Alg::X25519)?,
            queue_mode::decode_tail(d),
        ))
    }
}

impl SmpQueueUri {
    /// `strEncode`.
    #[must_use]
    pub fn to_uri(&self) -> String {
        let (vr, a) = (&self.client_v_range, &self.address);
        let mut params = vec![
            pair("v", range_str(vr)),
            pair("dh", key_str(&a.dh_public_key)),
        ];
        match a.queue_mode {
            Some(QueueMode::Messaging) => params.push(pair("q", "m")),
            Some(QueueMode::Contact) => params.push(pair("q", "c")),
            None => {}
        }
        if sender_can_secure(a.queue_mode) && vr.min() < SHORT_LINKS_CLIENT {
            params.push(pair("k", "s"));
        }
        let sid = b64_encode(&a.sender_id, true);
        if vr.min() >= SRV_HOSTNAMES_CLIENT {
            return format!("{}/{sid}#/?{}", a.server.to_uri(), query::render(&params));
        }
        if a.server.hosts.len() > 1 {
            params.push(pair("srv", a.server.hosts[1..].join(",")));
        }
        format!(
            "{}/{sid}#/?{}",
            a.server.to_legacy_uri(),
            query::render(&params)
        )
    }
}

impl Encode for SmpQueueUri {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let (vr, a) = (self.client_v_range, &self.address);
        (vr, &a.server, &a.sender_id, &a.dh_public_key).encode(out)?;
        let secure = sender_can_secure(a.queue_mode);
        if vr.min() >= SHORT_LINKS_CLIENT {
            return a.queue_mode.map_or(Ok(()), |m| m.encode(out));
        }
        // Older clients cannot ignore the Bool, so it is left out when it would be False.
        if vr.min() >= SND_AUTH_KEY_CLIENT || (vr.max() >= SND_AUTH_KEY_CLIENT && secure) {
            return secure.encode(out);
        }
        Ok(())
    }
}

impl Decode for SmpQueueUri {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (client_v_range, server) = <(VersionRange, SmpServer)>::decode(d)?;
        let (sender_id, dh_public_key, queue_mode) = SmpQueueAddress::decode_tail(d)?;
        let address = SmpQueueAddress {
            server,
            sender_id,
            dh_public_key,
            queue_mode,
        };
        Ok(Self {
            client_v_range,
            address,
        })
    }
}
