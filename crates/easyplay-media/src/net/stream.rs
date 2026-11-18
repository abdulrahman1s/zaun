//! A TURN server reached over TCP or TLS (RFC 8656 §3.1): messages back to back on one
//! stream, each framed by its own length. A reader thread hands whole messages over; the
//! session writes from its own thread.
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore};

use super::stun;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The largest message accepted (a datagram and its headers).
const MAX_FRAME: usize = 4096;

/// Where a stream's messages go; `None` once it closed.
pub(crate) type FrameSink = Box<dyn FnMut(Option<Vec<u8>>) + Send>;

/// TLS for `turns:` servers: the system-independent web roots, plus `extra` (tests).
pub(crate) fn tls_config(extra: &[CertificateDer<'static>]) -> io::Result<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for c in extra {
        roots.add(c.clone()).map_err(io::Error::other)?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

enum Kind {
    Tcp,
    Tls(Arc<Mutex<ClientConnection>>),
}

/// One stream to a TURN server.
pub(crate) struct Stream {
    sock: TcpStream,
    kind: Kind,
}

impl Stream {
    /// Connect (and for TLS, finish the handshake), then start reading into `sink`.
    pub fn connect(
        addr: SocketAddr,
        host: &str,
        tls: Option<Arc<ClientConfig>>,
        sink: FrameSink,
    ) -> io::Result<Self> {
        let mut sock = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
        sock.set_nodelay(true)?;
        let kind = match tls {
            None => Kind::Tcp,
            Some(config) => {
                let name = ServerName::try_from(host.to_owned()).map_err(io::Error::other)?;
                let mut conn = ClientConnection::new(config, name).map_err(io::Error::other)?;
                sock.set_read_timeout(Some(CONNECT_TIMEOUT))?;
                while conn.is_handshaking() {
                    conn.complete_io(&mut sock)?;
                }
                sock.set_read_timeout(None)?;
                Kind::Tls(Arc::new(Mutex::new(conn)))
            }
        };
        let reader = sock.try_clone()?;
        let conn = match &kind {
            Kind::Tcp => None,
            Kind::Tls(c) => Some(Arc::clone(c)),
        };
        std::thread::Builder::new()
            .name("call-relay".into())
            .spawn(move || read_loop(reader, conn, sink))?;
        Ok(Self { sock, kind })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.sock.local_addr()
    }

    pub fn send(&self, frame: &[u8]) -> io::Result<()> {
        let mut sock = &self.sock;
        match &self.kind {
            Kind::Tcp => sock.write_all(frame),
            Kind::Tls(conn) => {
                let mut c = conn.lock().unwrap_or_else(PoisonError::into_inner);
                c.writer().write_all(frame)?;
                while c.wants_write() {
                    c.write_tls(&mut sock)?;
                }
                Ok(())
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Ends the reader thread too.
        let _ = self.sock.shutdown(Shutdown::Both);
    }
}

fn read_loop(mut sock: TcpStream, conn: Option<Arc<Mutex<ClientConnection>>>, mut sink: FrameSink) {
    let mut buf = vec![0u8; 16 * 1024];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let n = match sock.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let ok = match &conn {
            None => {
                pending.extend_from_slice(&buf[..n]);
                true
            }
            Some(c) => tls_in(c, &mut sock, &buf[..n], &mut pending).is_ok(),
        };
        if !ok {
            break;
        }
        while let Some(len) = frame_len(&pending) {
            if len > MAX_FRAME {
                sink(None);
                return;
            }
            if pending.len() < len {
                break;
            }
            let frame: Vec<u8> = pending.drain(..len).collect();
            sink(Some(frame));
        }
    }
    sink(None);
}

/// Decrypt `data` into `plain`, answering what TLS wants to send meanwhile.
fn tls_in(
    conn: &Mutex<ClientConnection>,
    sock: &mut TcpStream,
    mut data: &[u8],
    plain: &mut Vec<u8>,
) -> io::Result<()> {
    let mut c = conn.lock().unwrap_or_else(PoisonError::into_inner);
    while !data.is_empty() {
        c.read_tls(&mut data)?;
        c.process_new_packets().map_err(io::Error::other)?;
        let mut chunk = [0u8; 4096];
        loop {
            match c.reader().read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => plain.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
    }
    while c.wants_write() {
        c.write_tls(sock)?;
    }
    Ok(())
}

/// The length on the wire of the message starting `buf`: a STUN message, or ChannelData
/// (padded to four bytes over a stream).
fn frame_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    if buf[0] & 0xC0 == 0x40 {
        let n = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
        return Some((4 + n).div_ceil(4) * 4);
    }
    stun::frame_len(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc::channel;

    #[test]
    fn messages_split_across_reads_arrive_whole_and_in_order() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let s = Stream::connect(
            addr,
            "localhost",
            None,
            Box::new(move |f| {
                let _ = tx.lock().unwrap().send(f);
            }),
        )
        .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let a =
            stun::Message::new(stun::BINDING, stun::Class::Success, [1; 12]).encode(None, false);
        let mut b = stun::Message::new(stun::DATA, stun::Class::Indication, [2; 12]);
        b.add(stun::DATA_ATTR, vec![7; 5]);
        let b = b.encode(None, false);
        let mut wire = a.clone();
        wire.extend_from_slice(&b);
        server.write_all(&wire[..7]).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        server.write_all(&wire[7..]).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Some(a));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Some(b));
        s.send(b"ping").unwrap();
        let mut got = [0u8; 4];
        server.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"ping");
        drop(server);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), None);
    }
}
