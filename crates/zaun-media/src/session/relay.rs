//! The call's TURN allocations: how each server is reached, and what goes through them.
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Instant;

use str0m::Candidate;

use super::run::{Fail, Session};
use crate::net::stream::Stream;
use crate::net::turn::{TurnClient, TurnEvent};

/// How a TURN server is reached.
pub(super) enum Link {
    /// Over the call's UDP socket.
    Udp(SocketAddr),
    Stream(Stream),
}

pub(super) struct Relay {
    pub client: TurnClient,
    pub link: Link,
    /// The address the allocation was asked from (for its candidate).
    pub local: SocketAddr,
}

impl Relay {
    pub fn put(&self, socket: &UdpSocket, bytes: &[u8]) {
        let r = match &self.link {
            Link::Udp(server) => socket.send_to(bytes, server).map(|_| ()),
            Link::Stream(s) => s.send(bytes),
        };
        if let Err(e) = r {
            tracing::debug!(error = %e, "call: a message to the relay was not sent");
        }
    }

    pub fn server_is(&self, from: SocketAddr) -> bool {
        matches!(self.link, Link::Udp(s) if s == from)
    }
}

impl Session {
    /// A message from relay `i`. Returns a new relay candidate once it is allocated.
    pub(super) fn relay_in(
        &mut self,
        i: usize,
        raw: &[u8],
        now: Instant,
    ) -> Result<Option<Candidate>, Fail> {
        let mut out = Vec::new();
        let Some(r) = self.relays.get_mut(i) else {
            return Ok(None);
        };
        let ev = r.client.handle(raw, now, &mut out);
        for b in &out {
            r.put(&self.socket, b);
        }
        match ev {
            None => Ok(None),
            Some(TurnEvent::Data { peer, data }) => {
                if let Some(relayed) = r.client.relayed() {
                    self.receive(peer, relayed, &data, now)?;
                }
                Ok(None)
            }
            Some(TurnEvent::Allocated { relayed }) => {
                tracing::info!(%relayed, "call: relay allocated");
                let local = r.local;
                let peers: Vec<IpAddr> = self.peers.iter().copied().collect();
                for ip in peers {
                    self.permit_on(i, ip, now);
                }
                Ok(Candidate::relayed(relayed, local, "udp").ok())
            }
            Some(TurnEvent::Failed(why)) => {
                self.relay_lost(&why)?;
                Ok(None)
            }
        }
    }

    /// A relay failed: without direct paths and without another relay, the call cannot go on.
    pub(super) fn relay_lost(&mut self, why: &str) -> Result<(), Fail> {
        tracing::warn!(why, "call: a relay failed");
        let any_left = self.relays.iter().any(|r| !r.client.failed());
        if self.host.is_none() && !any_left && self.described {
            return Err(Fail::call(format!("the call relay was lost: {why}")));
        }
        Ok(())
    }

    /// Let `ip` send to every relay address (a friend's candidate).
    pub(super) fn permit(&mut self, ip: IpAddr, now: Instant) {
        self.peers.insert(ip);
        for i in 0..self.relays.len() {
            self.permit_on(i, ip, now);
        }
    }

    fn permit_on(&mut self, i: usize, ip: IpAddr, now: Instant) {
        let Some(r) = self.relays.get_mut(i) else {
            return;
        };
        if let Some(b) = r.client.permit(ip, now) {
            r.put(&self.socket, &b);
        }
    }

    /// Retransmit and refresh on every relay.
    pub(super) fn relay_ticks(&mut self, now: Instant) -> Result<(), Fail> {
        let mut lost = None;
        for r in &mut self.relays {
            let mut out = Vec::new();
            if let Some(TurnEvent::Failed(why)) = r.client.tick(now, &mut out) {
                lost = Some(why);
            }
            for b in &out {
                r.put(&self.socket, b);
            }
        }
        match lost {
            Some(why) => self.relay_lost(&why),
            None => Ok(()),
        }
    }

    /// The earliest relay tick.
    pub(super) fn relay_deadline(&self) -> Option<Instant> {
        self.relays
            .iter()
            .filter_map(|r| r.client.next_tick())
            .min()
    }

    /// Release every allocation.
    pub(super) fn relays_close(&mut self) {
        for r in &mut self.relays {
            if let Some(b) = r.client.close() {
                r.put(&self.socket, &b);
            }
        }
    }
}
