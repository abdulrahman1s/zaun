//! The client side of the SMP handshake (Transport.hs `smpClientHandshake`, `smpTHandleClient`, `blockEncryption`; nook smp/handshake.ts).
//!
//! Server hello is read, checked against the TLS channel and the pinned identity, then the client hello is sent with
//! our X25519 key, as the Haskell client does (`agreeSecret = True`), which turns on sbc-chain block encryption.
use std::time::Duration;

use subtle::ConstantTimeEq;

use crate::crypto::{Alg, KeyPair, dh, key_hash};
use crate::encoding::{Version, VersionRange};
use crate::tls::{ChainCertificates, chain_id_ca_certs, verify_server_chain};

use super::block::{RcvBlocks, SndBlocks, block_ciphers};
use super::error::{HandshakeError, SmpError};
use super::hello::{ServerHello, encode_client_hello};
use super::signed_key::verify_signed_key;
use super::transport::{Established, Reader, Writer};
use super::version::{MAX_VERSION, MIN_VERSION};

/// Everything the handshake takes from the TLS layer.
pub struct TlsFacts<'a> {
    pub tls_unique: &'a [u8],
    pub peer_chain: &'a [Vec<u8>],
}

pub struct HandshakeConfig {
    pub range: VersionRange,
    pub block_encryption: bool,
    pub timeout: Duration,
}

impl HandshakeConfig {
    #[must_use]
    pub fn standard(timeout: Duration) -> Self {
        let range = VersionRange::new(Version(MIN_VERSION), Version(MAX_VERSION));
        Self {
            range: range.unwrap_or_else(|| VersionRange::single(Version(MAX_VERSION))),
            block_encryption: true,
            timeout,
        }
    }
}

/// Picks the highest version both sides speak.
pub fn negotiate(server: &VersionRange, ours: &VersionRange) -> Result<u16, HandshakeError> {
    server
        .intersect(ours)
        .map(|r| r.max().0)
        .ok_or(HandshakeError::Version)
}

pub fn client_handshake(
    reader: &mut dyn Reader,
    writer: &mut dyn Writer,
    tls: &TlsFacts<'_>,
    key_hash_of_server: &[u8],
    cfg: &HandshakeConfig,
) -> Result<Established, SmpError> {
    reader.set_read_timeout(Some(cfg.timeout))?;
    let hello = ServerHello::decode(&RcvBlocks::plain().read(reader)?)
        .map_err(|_| HandshakeError::Parse)?;
    if !bool::from(hello.session_id.as_slice().ct_eq(tls.tls_unique)) {
        return Err(HandshakeError::Session.into());
    }
    let version = negotiate(&hello.range, &cfg.range)?;
    verify_server_chain(tls.peer_chain, key_hash_of_server)
        .map_err(|_| HandshakeError::Identity)?;
    match chain_id_ca_certs(&hello.chain) {
        ChainCertificates::Valid { id, .. } if key_hash(id).as_slice() == key_hash_of_server => {}
        _ => return Err(HandshakeError::BadAuth.into()),
    }
    let leaf = tls.peer_chain.first().ok_or(HandshakeError::Identity)?;
    let server_key = verify_signed_key(leaf, &hello.signed_key)?;

    let ours = if cfg.block_encryption {
        Some(KeyPair::generate(Alg::X25519)?)
    } else {
        None
    };
    let hello_out = encode_client_hello(
        version,
        key_hash_of_server,
        ours.as_ref().map(|k| &k.public),
    )?;
    SndBlocks::plain().write(writer, &hello_out)?;

    let secret = ours
        .as_ref()
        .map(|k| dh(&server_key, &k.private))
        .transpose()?;
    let (snd, rcv) = block_ciphers(&hello.session_id, secret.as_ref())?;
    reader.set_read_timeout(None)?;
    Ok(Established {
        version,
        session_id: hello.session_id,
        server_key,
        server_info: hello.server_info,
        snd,
        rcv,
    })
}
