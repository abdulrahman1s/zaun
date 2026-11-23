//! Dialling an XFTP server: TCP, TLS 1.3 with ALPN `xftp/1`, HTTP/2, then the handshake (Client.hs `getXFTPClient`).
use std::net::{Shutdown, TcpStream};

use crate::h2::{Connection, Options};
use crate::net::connect_tcp;
use crate::smp::SystemClock;
use crate::tls::{TlsStream, verify_server_chain};

use super::address::XftpServer;
use super::client::{ClientConfig, XftpClient};
use super::error::{HandshakeError, XftpError};
use super::handshake::{Patience, TlsFacts, client_handshake};
use super::version::{ALPN, DEFAULT_PORT};

pub struct TcpXftpClient {
    pub client: XftpClient<TlsStream<TcpStream>>,
    socket: TcpStream,
}

fn connect_error(e: impl std::fmt::Display) -> XftpError {
    XftpError::Connect(e.to_string())
}

fn authority(host: &str, port: u16) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    if port == DEFAULT_PORT {
        host
    } else {
        format!("{host}:{port}")
    }
}

/// Tries each host of the address that is not a Tor host; the first that completes the handshake wins.
pub fn connect(server: &XftpServer, cfg: &ClientConfig) -> Result<TcpXftpClient, XftpError> {
    let mut last = XftpError::Connect("the address has no host that can be dialled".into());
    for host in server.dialable_hosts() {
        match connect_host(server, host, cfg) {
            Ok(c) => return Ok(c),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn connect_host(
    server: &XftpServer,
    host: &str,
    cfg: &ClientConfig,
) -> Result<TcpXftpClient, XftpError> {
    let port = server.port_or_default();
    let sock = connect_tcp(host, port, cfg.connect_timeout).map_err(connect_error)?;
    sock.set_read_timeout(Some(cfg.connect_timeout))?;
    let control = sock.try_clone()?;
    let tls = TlsStream::connect(sock, &[ALPN]).map_err(connect_error)?;
    control.set_read_timeout(Some(cfg.poll))?;
    control.set_write_timeout(Some(cfg.write_timeout))?;
    let tls_unique = *tls.tls_unique();
    let (chain, alpn) = (
        tls.peer_cert_chain().to_vec(),
        tls.info().alpn().map(str::to_owned),
    );
    let authority = authority(host, port);
    let mut conn = Connection::open(tls, Options::default())?;
    let facts = TlsFacts {
        tls_unique: &tls_unique,
        peer_chain: &chain,
    };
    // A server that does not speak the handshake is taken at version 1, as Haskell's client does.
    let (version, session_id) = if alpn.as_deref() == Some(ALPN) {
        let patience = Patience {
            clock: &SystemClock::new(),
            timeout: cfg.connect_timeout,
        };
        let n = client_handshake(&mut conn, &authority, &server.key_hash, &facts, &patience)?;
        (n.version, n.session_id)
    } else {
        verify_server_chain(&chain, &server.key_hash).map_err(|_| HandshakeError::Identity)?;
        (1, tls_unique.to_vec())
    };
    Ok(TcpXftpClient {
        client: XftpClient {
            conn,
            authority,
            session_id,
            version,
            cfg: *cfg,
        },
        socket: control,
    })
}

impl TcpXftpClient {
    /// GOAWAY, then the socket is shut down; the peer may be gone already.
    pub fn close(&mut self) {
        self.client.conn.close();
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}
