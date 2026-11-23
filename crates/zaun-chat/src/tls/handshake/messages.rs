//! Reading handshake messages out of plain and protected records during the handshake.
use std::io::Read;

use super::super::alert::Alert;
use super::super::error::TlsError;
use super::super::record::{ALERT, APPLICATION_DATA, CCS, HANDSHAKE, Inbound, Protection};
use super::super::wire::take_handshake_message;

/// A handshake message with its 4-byte header, as the transcript hashes it.
pub(super) struct Message {
    pub(super) ty: u8,
    pub(super) raw: Vec<u8>,
}

impl Message {
    pub(super) fn body(&self) -> &[u8] {
        &self.raw[4..]
    }
}

pub(super) struct Messages<'a, R: Read> {
    sock: &'a mut R,
    pub(super) inbound: Inbound,
    pub(super) rx: Option<Protection>,
    /// Handshake bytes read but not yet taken as messages.
    pub(super) pending: Vec<u8>,
}

impl<'a, R: Read> Messages<'a, R> {
    pub(super) fn new(sock: &'a mut R) -> Self {
        Self {
            sock,
            inbound: Inbound::default(),
            rx: None,
            pending: Vec::new(),
        }
    }

    pub(super) fn next_message(&mut self) -> Result<Message, TlsError> {
        loop {
            if let Some(m) = self.take() {
                return Ok(m);
            }
            self.read_record()?;
        }
    }

    pub(super) fn expect(&mut self, ty: u8, what: &'static str) -> Result<Message, TlsError> {
        let m = self.next_message()?;
        if m.ty == ty {
            Ok(m)
        } else {
            Err(TlsError::Protocol(what))
        }
    }

    fn take(&mut self) -> Option<Message> {
        take_handshake_message(&mut self.pending).map(|(ty, raw)| Message { ty, raw })
    }

    fn read_record(&mut self) -> Result<(), TlsError> {
        let record = self.inbound.next_record(self.sock)?;
        if record.header[0] == CCS {
            // Middlebox-compatibility ChangeCipherSpec (RFC 8446 appendix D.4).
            return if record.body == [1] {
                Ok(())
            } else {
                Err(TlsError::Illegal("ChangeCipherSpec"))
            };
        }
        let (ty, content) = match self.rx.as_mut() {
            None => (record.header[0], record.body),
            Some(rx) if record.header[0] == APPLICATION_DATA => {
                rx.open(&record.header, &record.body)?
            }
            Some(_) => return Err(TlsError::Protocol("unprotected record after keys")),
        };
        match ty {
            HANDSHAKE => self.pending.extend_from_slice(&content),
            ALERT => return Err(TlsError::Alert(Alert::parse(&content)?)),
            _ => return Err(TlsError::Protocol("record type during the handshake")),
        }
        Ok(())
    }
}
