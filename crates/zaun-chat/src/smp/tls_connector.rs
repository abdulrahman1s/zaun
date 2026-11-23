//! The production connector: TCP, TLS 1.3 with ALPN `smp/1`, then the SMP handshake (Transport/Client.hs `runTLSTransportClient`).
use std::io;
use std::net::{Shutdown, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crate::net::connect_tcp;
use crate::tls::{TlsReader, TlsStream, TlsWriter};

use super::address::SmpServer;
use super::config::ConnConfig;
use super::error::SmpError;
use super::handshake::{HandshakeConfig, TlsFacts, client_handshake};
use super::transport::{Closer, Connector, Reader, Wire, Writer};

pub struct TlsConnector;

struct Rd(TlsReader<TcpStream>);
struct Wr(TlsWriter<TcpStream>);
struct SocketCloser(TcpStream);

impl Reader for Rd {
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        io::Read::read_exact(&mut self.0, buf)
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }
}

impl Writer for Wr {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut self.0, buf)
    }
}

impl Closer for SocketCloser {
    fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both); // already closed is the goal
    }
}

fn io_err(e: impl std::fmt::Display) -> SmpError {
    SmpError::Io(e.to_string())
}

impl Connector for TlsConnector {
    fn connect(
        &self,
        server: &SmpServer,
        host: &str,
        cfg: &ConnConfig,
        closer_slot: &dyn Fn(Arc<dyn Closer>),
    ) -> Result<Wire, SmpError> {
        let sock =
            connect_tcp(host, server.port_or_default(), cfg.connect_timeout).map_err(io_err)?;
        sock.set_read_timeout(Some(cfg.connect_timeout))?;
        let closer: Arc<dyn Closer> = Arc::new(SocketCloser(sock.try_clone()?));
        closer_slot(Arc::clone(&closer));
        let tls = TlsStream::connect(sock, &["smp/1"]).map_err(io_err)?;
        let (tls_unique, peer_chain) = (*tls.tls_unique(), tls.peer_cert_chain().to_vec());
        let (r, w) = tls.split();
        let (mut rd, mut wr) = (Rd(r), Wr(w));
        let facts = TlsFacts {
            tls_unique: &tls_unique,
            peer_chain: &peer_chain,
        };
        let hs = HandshakeConfig {
            range: cfg.versions,
            block_encryption: cfg.block_encryption,
            timeout: cfg.connect_timeout,
        };
        let session = client_handshake(&mut rd, &mut wr, &facts, &server.key_hash, &hs)?;
        Ok(Wire {
            reader: Box::new(rd),
            writer: Box::new(wr),
            closer,
            session,
        })
    }
}
