//! Gathering: the host address and the address seen from outside (direct calls only), and an
//! allocation on each TURN server, all before the description is written (the engine
//! carries one description; later candidates would need `/_call extra`).
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use rustls::ClientConfig;
use str0m::Candidate;
use zaun_core::chat::media::{IceKind, IceServer, IceTransport, MediaConfig};

use super::inbox::Inbox;
use super::relay::{Link, Relay};
use super::run::{Fail, Session};
use crate::clock::now;
use crate::net::stream::Stream;
use crate::net::stun::{self, Class, Message};
use crate::net::turn::TurnClient;

/// The longest gathering waits for servers that do not answer.
const GATHER_FOR: Duration = Duration::from_secs(5);

impl Session {
    pub(super) fn gather(
        &mut self,
        config: &MediaConfig,
        tls: Option<&Arc<ClientConfig>>,
    ) -> Result<Vec<Candidate>, Fail> {
        let mut found = Vec::new();
        if let Some(h) = self.host {
            found.extend(Candidate::host(h, "udp").ok());
        }
        let mut binding = Vec::new();
        for s in &config.ice {
            let Some(addr) = resolve(s) else {
                tracing::info!(host = %s.host, "call: an ICE server's name did not resolve");
                continue;
            };
            match (s.kind, s.transport) {
                (IceKind::Stun, IceTransport::Udp) if self.host.is_some() => {
                    let m = Message::new(stun::BINDING, Class::Request, stun::tid());
                    if self.socket.send_to(&m.encode(None, true), addr).is_ok() {
                        binding.push((m.tid, addr));
                    }
                }
                (IceKind::Stun, _) => {}
                (IceKind::Turn, transport) => self.allocate(s, addr, transport, tls),
            }
        }
        let deadline = now() + GATHER_FOR;
        loop {
            let waiting = !binding.is_empty()
                || self
                    .relays
                    .iter()
                    .any(|r| r.client.relayed().is_none() && !r.client.failed());
            let left = deadline.saturating_duration_since(now());
            if !waiting || left.is_zero() {
                break;
            }
            let wait = self
                .relay_deadline()
                .map_or(left, |d| d.saturating_duration_since(now()).min(left))
                .max(Duration::from_millis(1));
            match self.rx.recv_timeout(wait) {
                Ok(Inbox::Udp { from, data }) => {
                    if let Some(c) = self.binding_answer(&mut binding, from, &data) {
                        found.push(c);
                    } else if let Some(i) = self.relays.iter().position(|r| r.server_is(from)) {
                        found.extend(self.relay_in(i, &data, now())?);
                    }
                }
                Ok(Inbox::Stream { relay, frame }) => match frame {
                    Some(f) => found.extend(self.relay_in(relay, &f, now())?),
                    None => {
                        if let Some(r) = self.relays.get_mut(relay) {
                            r.client.abandon();
                        }
                        tracing::info!("call: a relay connection closed while gathering");
                    }
                },
                Ok(Inbox::Frame { level, .. }) => self.me = level,
                Ok(Inbox::Failed { code, detail }) => return Err(Fail::Error { code, detail }),
                Ok(Inbox::Stop) => return Err(Fail::Stopped),
                Ok(msg) => self.early.push(msg),
                Err(RecvTimeoutError::Timeout) => self.relay_ticks(now())?,
                Err(RecvTimeoutError::Disconnected) => return Err(Fail::Stopped),
            }
        }
        Ok(found)
    }

    /// Ask TURN server `s` (at `addr`) for an allocation.
    fn allocate(
        &mut self,
        s: &IceServer,
        addr: SocketAddr,
        transport: IceTransport,
        tls: Option<&Arc<ClientConfig>>,
    ) {
        let index = self.relays.len();
        let (link, local, reliable) = match transport {
            IceTransport::Udp => {
                let local = self.socket.local_addr().unwrap_or(addr);
                (Link::Udp(addr), local, false)
            }
            IceTransport::Tcp | IceTransport::Tls => {
                let tx = std::sync::Mutex::new(self.tx.clone());
                let sink = Box::new(move |frame: Option<Vec<u8>>| {
                    let tx = tx.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let _ = tx.send(Inbox::Stream {
                        relay: index,
                        frame,
                    });
                });
                let tls = (transport == IceTransport::Tls)
                    .then(|| tls.cloned())
                    .flatten();
                if transport == IceTransport::Tls && tls.is_none() {
                    tracing::warn!("call: no TLS for a turns: server");
                    return;
                }
                match Stream::connect(addr, &s.host, tls, sink) {
                    Ok(st) => {
                        let local = st.local_addr().unwrap_or(addr);
                        (Link::Stream(st), local, true)
                    }
                    Err(e) => {
                        tracing::info!(error = %e, host = %s.host, "call: the relay could not be reached");
                        return;
                    }
                }
            }
        };
        let mut client = TurnClient::new(&s.username, &s.credential, reliable);
        let first = client.start(now());
        let relay = Relay {
            client,
            link,
            local,
        };
        relay.put(&self.socket, &first);
        self.relays.push(relay);
    }

    /// A STUN server's answer: this computer's address as seen from outside.
    fn binding_answer(
        &self,
        waiting: &mut Vec<([u8; 12], SocketAddr)>,
        from: SocketAddr,
        data: &[u8],
    ) -> Option<Candidate> {
        let m = Message::decode(data)?;
        let i = waiting
            .iter()
            .position(|(tid, addr)| *tid == m.tid && *addr == from)?;
        waiting.remove(i);
        let mapped = m.xor_addr(stun::XOR_MAPPED_ADDRESS)?;
        let base = self.host?;
        (mapped != base)
            .then(|| Candidate::server_reflexive(mapped, base, "udp").ok())
            .flatten()
    }
}

/// The server's IPv4 address (the call socket is IPv4).
fn resolve(s: &IceServer) -> Option<SocketAddr> {
    (s.host.as_str(), s.port)
        .to_socket_addrs()
        .ok()?
        .find(SocketAddr::is_ipv4)
}
