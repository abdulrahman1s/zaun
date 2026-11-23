//! The XFTP handshake over HTTP/2: a POST for the server's block, checks against TLS and the pinned identity, a POST for ours (Client.hs `xftpClientHandshakeV1`).
use std::io::{Read, Write};
use std::time::Duration;

use subtle::ConstantTimeEq;

use crate::encoding::{Version, VersionRange};
use crate::h2::{Connection, Progress, Request, Stop};
use crate::smp::Clock;
use crate::tls::verify_server_chain;

use super::error::{HandshakeError, XftpError};
use super::handshake_msgs::{ServerHandshake, encode_client_handshake};
use super::signed_key::verify_signed_key;
use super::version::{BLOCK_SIZE, MAX_VERSION, MIN_VERSION};

/// What the handshake takes from the TLS layer.
pub struct TlsFacts<'a> {
    pub tls_unique: &'a [u8],
    pub peer_chain: &'a [Vec<u8>],
}

pub struct Negotiated {
    pub version: u16,
    pub session_id: Vec<u8>,
}

/// How long each of the two handshake requests may take; the clock is `crate::smp::Clock`.
pub struct Patience<'a> {
    pub clock: &'a dyn Clock,
    pub timeout: Duration,
}

fn post<T: Read + Write>(
    conn: &mut Connection<T>,
    authority: &str,
    body: &[u8],
    patience: &Patience<'_>,
) -> Result<Vec<u8>, XftpError> {
    let req = Request {
        method: "POST",
        path: "/",
        authority,
        headers: &[],
        body,
    };
    let timeout = u64::try_from(patience.timeout.as_millis()).unwrap_or(u64::MAX);
    let until = patience.clock.now_ms().saturating_add(timeout);
    let mut watch = |_: Progress| -> Result<(), Stop> {
        if patience.clock.now_ms() > until {
            Err(Stop::TimedOut)
        } else {
            Ok(())
        }
    };
    let resp = conn.request(&req, BLOCK_SIZE, &mut watch)?;
    if resp.status != 200 {
        return Err(HandshakeError::Status(resp.status).into());
    }
    Ok(resp.body)
}

pub fn client_handshake<T: Read + Write>(
    conn: &mut Connection<T>,
    authority: &str,
    key_hash: &[u8],
    tls: &TlsFacts<'_>,
    patience: &Patience<'_>,
) -> Result<Negotiated, XftpError> {
    verify_server_chain(tls.peer_chain, key_hash).map_err(|_| HandshakeError::Identity)?;
    let hello = ServerHandshake::decode_block(&post(conn, authority, &[], patience)?)?;
    if !bool::from(hello.session_id.as_slice().ct_eq(tls.tls_unique)) {
        return Err(HandshakeError::Session.into());
    }
    let ours = VersionRange::new(Version(MIN_VERSION), Version(MAX_VERSION))
        .ok_or(HandshakeError::Version)?;
    let version = hello
        .range
        .intersect(&ours)
        .ok_or(HandshakeError::Version)?
        .max()
        .0;
    if hello.chain.as_slice() != tls.peer_chain {
        return Err(HandshakeError::BadAuth.into());
    }
    verify_signed_key(&hello.chain[0], &hello.signed_key)?;
    let ack = post(
        conn,
        authority,
        &encode_client_handshake(version, key_hash)?,
        patience,
    )?;
    if !ack.is_empty() {
        return Err(HandshakeError::Parse.into());
    }
    Ok(Negotiated {
        version,
        session_id: hello.session_id,
    })
}
