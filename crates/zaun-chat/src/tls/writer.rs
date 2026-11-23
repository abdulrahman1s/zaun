//! The sending half of a TLS connection: its own key, sequence number and socket handle.
use std::io::{self, Write};
use std::net::Shutdown;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::alert::Alert;
use super::error::TlsError;
use super::info::TlsInfo;
use super::key_schedule::{Secret, next_traffic_secret};
use super::record::{ALERT, APPLICATION_DATA, HANDSHAKE, MAX_PLAINTEXT, Protection};
use super::shared::Shared;
use super::wire::handshake_message;
use crate::net::Socket;

const KEY_UPDATE: u8 = 24;

pub struct TlsWriter<S: Socket> {
    sock: S,
    tx: Protection,
    secret: Secret,
    shared: Arc<Shared>,
}

impl<S: Socket> TlsWriter<S> {
    pub(super) fn new(sock: S, tx: Protection, secret: Secret, shared: Arc<Shared>) -> Self {
        Self {
            sock,
            tx,
            secret,
            shared,
        }
    }

    #[must_use]
    pub fn info(&self) -> &TlsInfo {
        &self.shared.info
    }

    /// Rotates our sending key and tells the server to do the same for what it sends.
    pub fn request_key_update(&mut self) -> Result<(), TlsError> {
        let record = self.rotate_keys(true)?;
        self.sock.write_all(&record)?;
        Ok(())
    }

    /// Sends close_notify and shuts down the sending direction.
    pub fn close(&mut self) -> Result<(), TlsError> {
        let record = self.tx.seal(ALERT, &Alert::CLOSE_NOTIFY.bytes())?;
        self.sock.write_all(&record)?;
        self.sock.shutdown(Shutdown::Write)?;
        Ok(())
    }

    /// The KeyUpdate record under the old key; later records use the next key.
    fn rotate_keys(&mut self, request_peer: bool) -> Result<Vec<u8>, TlsError> {
        let message = handshake_message(KEY_UPDATE, &[u8::from(request_peer)]);
        let record = self.tx.seal(HANDSHAKE, &message)?;
        self.secret = next_traffic_secret(&self.secret);
        self.tx = Protection::new(&self.secret);
        Ok(record)
    }

    fn send(&mut self, data: &[u8]) -> Result<(), TlsError> {
        let mut out = Vec::with_capacity(data.len() + 64);
        if self
            .shared
            .key_update_requested
            .swap(false, Ordering::AcqRel)
        {
            out.extend(self.rotate_keys(false)?);
        }
        for chunk in data.chunks(MAX_PLAINTEXT) {
            out.extend(self.tx.seal(APPLICATION_DATA, chunk)?);
        }
        self.sock.write_all(&out)?;
        Ok(())
    }
}

impl<S: Socket> Write for TlsWriter<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.send(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sock.flush()
    }
}
