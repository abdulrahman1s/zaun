//! TCP connect to a host:port, trying every resolved address (SMP address `host:port`).
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::NetError;

/// No SNI and no TLS here; `timeout` bounds each address attempt.
pub fn connect_tcp(host: &str, port: u16, timeout: Duration) -> Result<TcpStream, NetError> {
    if host.ends_with(".onion") {
        return Err(NetError::OnionUnsupported(host.to_owned()));
    }
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|source| NetError::Resolve {
            host: host.to_owned(),
            source,
        })?;
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => {
                stream.set_nodelay(true).ok(); // a latency hint only
                return Ok(stream);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(NetError::Connect {
        host: host.to_owned(),
        port,
        source: last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no addresses")),
    })
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn onion_hosts_are_refused_before_resolving() {
        let r = connect_tcp("abcdefg.onion", 5223, Duration::from_secs(1));
        assert!(matches!(r, Err(NetError::OnionUnsupported(_))));
    }

    #[test]
    fn connects_to_a_listening_loopback_port() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(connect_tcp("127.0.0.1", port, Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn a_refused_port_reports_the_connect_error() {
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let r = connect_tcp("127.0.0.1", port, Duration::from_secs(2));
        assert!(matches!(r, Err(NetError::Connect { .. })));
    }
}
