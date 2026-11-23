//! The receiving half of a TLS connection: its own key, sequence number and socket handle.
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::alert::Alert;
use super::error::TlsError;
use super::info::TlsInfo;
use super::key_schedule::{Secret, next_traffic_secret};
use super::record::{ALERT, APPLICATION_DATA, HANDSHAKE, Inbound, Protection};
use super::shared::Shared;
use super::wire::take_handshake_message;
use crate::net::Socket;

const NEW_SESSION_TICKET: u8 = 4;
const KEY_UPDATE: u8 = 24;

/// Reads application data. `Ok(0)` means the server sent close_notify. A read timeout leaves
/// the connection usable: call `read` again.
pub struct TlsReader<S: Socket> {
    sock: S,
    inbound: Inbound,
    rx: Protection,
    secret: Secret,
    handshake_buf: Vec<u8>,
    app: Vec<u8>,
    app_pos: usize,
    closed: bool,
    shared: Arc<Shared>,
}

impl<S: Socket> TlsReader<S> {
    pub(super) fn new(
        sock: S,
        inbound: Inbound,
        rx: Protection,
        secret: Secret,
        handshake_buf: Vec<u8>,
        shared: Arc<Shared>,
    ) -> Result<Self, TlsError> {
        let mut reader = Self {
            sock,
            inbound,
            rx,
            secret,
            handshake_buf,
            app: Vec::new(),
            app_pos: 0,
            closed: false,
            shared,
        };
        reader.process_handshake_messages()?;
        Ok(reader)
    }

    #[must_use]
    pub fn info(&self) -> &TlsInfo {
        &self.shared.info
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.sock.set_read_timeout(timeout)
    }

    fn pump(&mut self) -> Result<(), TlsError> {
        let record = self.inbound.next_record(&mut self.sock)?;
        if record.header[0] != APPLICATION_DATA {
            return Err(TlsError::Protocol("unprotected record after the handshake"));
        }
        let (ty, content) = self.rx.open(&record.header, &record.body)?;
        match ty {
            APPLICATION_DATA => {
                self.app = content;
                self.app_pos = 0;
            }
            ALERT => {
                let alert = Alert::parse(&content)?;
                if !alert.is_close_notify() {
                    return Err(TlsError::Alert(alert));
                }
                self.closed = true;
            }
            HANDSHAKE => {
                self.handshake_buf.extend_from_slice(&content);
                self.process_handshake_messages()?;
            }
            _ => return Err(TlsError::Protocol("record type")),
        }
        Ok(())
    }

    fn process_handshake_messages(&mut self) -> Result<(), TlsError> {
        while let Some((ty, raw)) = take_handshake_message(&mut self.handshake_buf) {
            match ty {
                NEW_SESSION_TICKET => {}
                KEY_UPDATE => self.key_update(&raw[4..])?,
                _ => return Err(TlsError::Protocol("unexpected post-handshake message")),
            }
        }
        Ok(())
    }

    fn key_update(&mut self, body: &[u8]) -> Result<(), TlsError> {
        let requested = match body {
            [0] => false,
            [1] => true,
            _ => return Err(TlsError::Illegal("KeyUpdate request")),
        };
        self.secret = next_traffic_secret(&self.secret);
        self.rx = Protection::new(&self.secret);
        if requested {
            self.shared
                .key_update_requested
                .store(true, Ordering::Release);
        }
        Ok(())
    }
}

impl<S: Socket> Read for TlsReader<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let available = &self.app[self.app_pos..];
            if !available.is_empty() {
                let n = available.len().min(buf.len());
                buf[..n].copy_from_slice(&available[..n]);
                self.app_pos += n;
                return Ok(n);
            }
            if self.closed {
                return Ok(0);
            }
            self.pump()?;
        }
    }
}
