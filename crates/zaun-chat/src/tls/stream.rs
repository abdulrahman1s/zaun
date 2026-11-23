//! A TLS 1.3 client connection over any `Socket`, whole or split into reader and writer halves.
use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::error::TlsError;
use super::handshake::perform;
use super::info::TlsInfo;
use super::reader::TlsReader;
use super::shared::Shared;
use super::writer::TlsWriter;
use crate::net::Socket;

pub struct TlsStream<S: Socket> {
    reader: TlsReader<S>,
    writer: TlsWriter<S>,
}

impl<S: Socket> TlsStream<S> {
    /// Runs the handshake on `sock`, offering `alpn` (`smp/1`, `xftp/1`; empty offers none).
    /// Set a read timeout on the socket first to bound the handshake.
    pub fn connect(mut sock: S, alpn: &[&str]) -> Result<Self, TlsError> {
        let est = perform(&mut sock, alpn)?;
        let writer_sock = sock.try_clone()?;
        let shared = Arc::new(Shared {
            info: est.info,
            key_update_requested: AtomicBool::new(false),
        });
        let reader = TlsReader::new(
            sock,
            est.inbound,
            est.rx,
            est.rx_secret,
            est.pending,
            Arc::clone(&shared),
        )?;
        let writer = TlsWriter::new(writer_sock, est.tx, est.tx_secret, shared);
        Ok(Self { reader, writer })
    }

    #[must_use]
    pub fn info(&self) -> &TlsInfo {
        self.reader.info()
    }

    /// The client Finished verify_data; SMP's session id.
    #[must_use]
    pub fn tls_unique(&self) -> &[u8; 32] {
        self.info().tls_unique()
    }

    /// DER certificates the server sent, leaf first.
    #[must_use]
    pub fn peer_cert_chain(&self) -> &[Vec<u8>] {
        self.info().peer_cert_chain()
    }

    /// Two halves for two threads; each has its own key and sequence number.
    #[must_use]
    pub fn split(self) -> (TlsReader<S>, TlsWriter<S>) {
        (self.reader, self.writer)
    }

    pub fn close(&mut self) -> Result<(), TlsError> {
        self.writer.close()
    }
}

impl<S: Socket> Read for TlsStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl<S: Socket> Write for TlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
