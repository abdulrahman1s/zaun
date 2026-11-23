//! The byte pipe under one SMP session and how it is opened; a seam so the connection can run against an in-memory server.
use std::io;
use std::sync::Arc;
use std::time::Duration;

use super::address::SmpServer;
use super::block::{RcvBlocks, SndBlocks};
use super::config::ConnConfig;
use super::error::SmpError;
use crate::crypto::PublicKey;

pub trait Reader: Send {
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()>;
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
}

pub trait Writer: Send {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()>;
}

/// Shuts the connection from any thread, which unblocks a reader stuck in `read_exact`.
pub trait Closer: Send + Sync {
    fn close(&self);
}

/// What the handshake settled (Transport.hs `THandleParams`).
pub struct Established {
    pub version: u16,
    pub session_id: Vec<u8>,
    /// The server's signed X25519 key, the DH half of the X25519 command authenticator.
    pub server_key: PublicKey,
    pub server_info: Option<Vec<u8>>,
    pub snd: SndBlocks,
    pub rcv: RcvBlocks,
}

/// A connected, handshaken session ready to be split between a reader and a writer thread.
pub struct Wire {
    pub reader: Box<dyn Reader>,
    pub writer: Box<dyn Writer>,
    pub closer: Arc<dyn Closer>,
    pub session: Established,
}

/// Opens one session to `host` of `server`: TCP, TLS and the SMP handshake in production.
pub trait Connector: Send + Sync {
    /// `closer_slot` receives the closer as soon as it exists, so a stalled connect can be cancelled.
    fn connect(
        &self,
        server: &SmpServer,
        host: &str,
        cfg: &ConnConfig,
        closer_slot: &dyn Fn(Arc<dyn Closer>),
    ) -> Result<Wire, SmpError>;
}
