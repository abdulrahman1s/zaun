//! A connection's receive and send queue as stored (Agent/Store.hs `RcvQueue`, `SndQueue`, AGPL-3.0-only).
use crate::crypto::{DhSecret, PrivateKey, PublicKey, dh};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Version};
use crate::smp::SmpServer;

use crate::agent::error::{AgentError, Result};

/// Our queue: the peer sends to `snd_id`, we read `rcv_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcvQueue {
    pub server: SmpServer,
    /// Empty until the server answers NEW.
    pub rcv_id: Vec<u8>,
    pub snd_id: Vec<u8>,
    /// Ed25519; signs SUB, ACK and DEL.
    pub auth: PrivateKey,
    /// X25519; with `server_dh` it opens the server-to-recipient layer.
    pub dh_priv: PrivateKey,
    pub server_dh: Option<PublicKey>,
    /// X25519; its public half is the `dh=` of our link or reply-queue info.
    pub e2e_priv: PrivateKey,
    /// The sender's key from its confirmation; set once the queue is confirmed.
    pub peer_e2e: Option<PublicKey>,
    pub client_version: Version,
}

/// The peer's queue we send to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SndQueue {
    pub server: SmpServer,
    pub snd_id: Vec<u8>,
    /// X25519 as in 7.1.4 (`sndAuthAlg`): commands carry a `cbAuthenticate` box, not a signature.
    pub auth: PrivateKey,
    pub e2e_priv: PrivateKey,
    /// The recipient's e2e key from its link or reply-queue info.
    pub peer_dh: PublicKey,
    pub client_version: Version,
    /// SKEY or LKEY has succeeded.
    pub secured: bool,
}

impl RcvQueue {
    /// The server-to-recipient secret, once the server has answered NEW.
    pub fn server_secret(&self) -> Result<DhSecret> {
        let key = self
            .server_dh
            .as_ref()
            .ok_or(AgentError::Prohibited("queue not created"))?;
        Ok(dh(key, &self.dh_priv)?)
    }
}

impl SndQueue {
    pub fn e2e_secret(&self) -> Result<DhSecret> {
        Ok(dh(&self.peer_dh, &self.e2e_priv)?)
    }
}

impl Encode for RcvQueue {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.server, &self.rcv_id, &self.snd_id, &self.auth).encode(out)?;
        (
            &self.dh_priv,
            &self.server_dh,
            &self.e2e_priv,
            &self.peer_e2e,
        )
            .encode(out)?;
        self.client_version.encode(out)
    }
}

impl Decode for RcvQueue {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (server, rcv_id, snd_id, auth) = <_>::decode(d)?;
        let (dh_priv, server_dh, e2e_priv, peer_e2e) = <_>::decode(d)?;
        Ok(Self {
            server,
            rcv_id,
            snd_id,
            auth,
            dh_priv,
            server_dh,
            e2e_priv,
            peer_e2e,
            client_version: Version::decode(d)?,
        })
    }
}

impl Encode for SndQueue {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.server, &self.snd_id, &self.auth, &self.e2e_priv).encode(out)?;
        (&self.peer_dh, self.client_version, self.secured).encode(out)
    }
}

impl Decode for SndQueue {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (server, snd_id, auth, e2e_priv) = <_>::decode(d)?;
        let (peer_dh, client_version, secured) = <_>::decode(d)?;
        Ok(Self {
            server,
            snd_id,
            auth,
            e2e_priv,
            peer_dh,
            client_version,
            secured,
        })
    }
}
