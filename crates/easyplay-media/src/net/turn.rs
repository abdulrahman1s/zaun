//! A small TURN client (RFC 8656), sans-IO: it writes and reads messages and the session
//! carries them over UDP, TCP or TLS. Long-term credentials, Allocate, CreatePermission,
//! Send and Data indications, Refresh; a stale nonce is retried once per request.
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use super::stun::{self, Class, Message};

/// UDP to the peer (REQUESTED-TRANSPORT).
const UDP: u8 = 17;
/// Permissions last five minutes; they are renewed after four.
const PERMISSION_RENEW: Duration = Duration::from_secs(240);
/// The first retransmission over UDP (doubled each time, four tries).
const RTO: Duration = Duration::from_millis(500);
const TRIES: u32 = 4;

/// What the client reports.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TurnEvent {
    /// The relay address the server gave.
    Allocated { relayed: SocketAddr },
    /// A datagram a peer sent to the relay address.
    Data { peer: SocketAddr, data: Vec<u8> },
    /// The allocation failed or was lost.
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Allocate,
    Permission(IpAddr),
    Refresh,
}

struct Pending {
    kind: Kind,
    message: Message,
    sent: Instant,
    tries: u32,
    /// Retried once already after a 401/438.
    reauth: bool,
}

/// One allocation on one TURN server.
pub(crate) struct TurnClient {
    username: String,
    password: String,
    realm: Option<String>,
    nonce: Option<String>,
    key: Option<[u8; 16]>,
    /// TCP or TLS: the transport retransmits.
    reliable: bool,
    relayed: Option<SocketAddr>,
    refresh_at: Option<Instant>,
    failed: bool,
    pending: HashMap<[u8; 12], Pending>,
    /// Permitted peer addresses and when each was last renewed.
    permissions: HashMap<IpAddr, Instant>,
}

impl TurnClient {
    pub fn new(username: &str, password: &str, reliable: bool) -> Self {
        Self {
            username: username.to_owned(),
            password: password.to_owned(),
            realm: None,
            nonce: None,
            key: None,
            reliable,
            relayed: None,
            refresh_at: None,
            failed: false,
            pending: HashMap::new(),
            permissions: HashMap::new(),
        }
    }

    pub fn relayed(&self) -> Option<SocketAddr> {
        self.relayed
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    /// The first Allocate (without credentials: the server answers with its realm and nonce).
    pub fn start(&mut self, now: Instant) -> Vec<u8> {
        let mut m = Message::new(stun::ALLOCATE, Class::Request, stun::tid());
        m.add(stun::REQUESTED_TRANSPORT, vec![UDP, 0, 0, 0]);
        self.request(Kind::Allocate, m, now)
    }

    /// Permit `ip` to send to the relay address (renewed when older than four minutes).
    pub fn permit(&mut self, ip: IpAddr, now: Instant) -> Option<Vec<u8>> {
        // Before the allocation, nothing: the session asks again once allocated.
        self.relayed?;
        let fresh = self
            .permissions
            .get(&ip)
            .is_some_and(|t| now.saturating_duration_since(*t) < PERMISSION_RENEW);
        if fresh {
            return None;
        }
        self.permissions.insert(ip, now);
        let mut m = Message::new(stun::CREATE_PERMISSION, Class::Request, stun::tid());
        m.add_xor_addr(stun::XOR_PEER_ADDRESS, SocketAddr::new(ip, 0));
        Some(self.request(Kind::Permission(ip), m, now))
    }

    /// `data` for `peer`, as a Send indication.
    pub fn send(&self, peer: SocketAddr, data: &[u8]) -> Vec<u8> {
        let mut m = Message::new(stun::SEND, Class::Indication, stun::tid());
        m.add_xor_addr(stun::XOR_PEER_ADDRESS, peer);
        m.add(stun::DATA_ATTR, data);
        m.encode(None, false)
    }

    /// Release the allocation (Refresh with a lifetime of zero); nothing to answer.
    pub fn close(&mut self) -> Option<Vec<u8>> {
        self.relayed.take()?;
        let mut m = Message::new(stun::REFRESH, Class::Request, stun::tid());
        m.add(stun::LIFETIME, 0u32.to_be_bytes().to_vec());
        Some(self.authed(&mut m))
    }

    /// The connection to the server is gone: the allocation is lost (nothing to send).
    pub fn abandon(&mut self) {
        self.fail("the connection to the relay closed");
    }

    /// When [`TurnClient::tick`] wants to run next.
    pub fn next_tick(&self) -> Option<Instant> {
        let retry = (!self.reliable)
            .then(|| {
                self.pending
                    .values()
                    .map(|p| p.sent + RTO * 2u32.pow(p.tries - 1))
                    .min()
            })
            .flatten();
        let permits = self.relayed.and_then(|_| {
            self.permissions
                .values()
                .map(|t| *t + PERMISSION_RENEW)
                .min()
        });
        [retry, self.refresh_at, permits]
            .into_iter()
            .flatten()
            .min()
    }

    /// Retransmit over UDP, refresh the allocation and the permissions.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) -> Option<TurnEvent> {
        if !self.reliable {
            let mut lost = None;
            let key = self.key;
            for p in self.pending.values_mut() {
                if now < p.sent + RTO * 2u32.pow(p.tries - 1) {
                    continue;
                }
                if p.tries >= TRIES {
                    lost.get_or_insert(p.kind);
                    continue;
                }
                p.tries += 1;
                p.sent = now;
                out.push(p.message.encode(key.as_ref().map(|k| &k[..]), false));
            }
            if let Some(kind) = lost {
                self.pending.retain(|_, p| p.tries < TRIES);
                if kind == Kind::Allocate || kind == Kind::Refresh {
                    return Some(self.fail("the relay did not answer"));
                }
            }
        }
        if self.refresh_at.is_some_and(|t| now >= t) {
            self.refresh_at = None;
            let mut m = Message::new(stun::REFRESH, Class::Request, stun::tid());
            m.add(stun::LIFETIME, 600u32.to_be_bytes().to_vec());
            out.push(self.request(Kind::Refresh, m, now));
        }
        // Each permission is renewed on its own, a minute before it would lapse.
        let due: Vec<IpAddr> = self
            .permissions
            .iter()
            .filter(|(_, t)| now >= **t + PERMISSION_RENEW)
            .map(|(ip, _)| *ip)
            .collect();
        for ip in due {
            out.extend(self.permit(ip, now));
        }
        None
    }

    /// A message from the server.
    pub fn handle(
        &mut self,
        raw: &[u8],
        now: Instant,
        out: &mut Vec<Vec<u8>>,
    ) -> Option<TurnEvent> {
        let m = Message::decode(raw)?;
        if m.class == Class::Indication {
            if m.method != stun::DATA {
                return None;
            }
            let peer = m.xor_addr(stun::XOR_PEER_ADDRESS)?;
            let data = m.get(stun::DATA_ATTR)?.to_vec();
            return Some(TurnEvent::Data { peer, data });
        }
        let p = self.pending.remove(&m.tid)?;
        if let Some(key) = self.key
            && m.class == Class::Success
            && !stun::check_integrity(raw, &key)
        {
            tracing::warn!("call: a relay answer failed its integrity check");
            self.pending.insert(m.tid, p);
            return None;
        }
        match m.class {
            Class::Success => self.on_success(p.kind, &m, now),
            Class::Error => self.on_error(p, &m, now, out),
            Class::Request | Class::Indication => None,
        }
    }

    fn on_success(&mut self, kind: Kind, m: &Message, now: Instant) -> Option<TurnEvent> {
        match kind {
            Kind::Allocate => {
                let relayed = m.xor_addr(stun::XOR_RELAYED_ADDRESS)?;
                self.relayed = Some(relayed);
                self.schedule_refresh(m, now);
                Some(TurnEvent::Allocated { relayed })
            }
            Kind::Refresh => {
                self.schedule_refresh(m, now);
                None
            }
            Kind::Permission(_) => None,
        }
    }

    fn on_error(
        &mut self,
        p: Pending,
        m: &Message,
        now: Instant,
        out: &mut Vec<Vec<u8>>,
    ) -> Option<TurnEvent> {
        let code = m.error_code().unwrap_or(0);
        if matches!(code, 401 | 438) && !p.reauth {
            if let Some(realm) = m.text(stun::REALM) {
                self.realm = Some(realm);
            }
            self.nonce = m.text(stun::NONCE).or(self.nonce.take());
            if let Some(realm) = &self.realm {
                self.key = Some(stun::long_term_key(&self.username, realm, &self.password));
            }
            let mut again = p.message;
            again.tid = stun::tid();
            let tid = again.tid;
            let bytes = self.authed(&mut again);
            self.pending.insert(
                tid,
                Pending {
                    kind: p.kind,
                    message: again,
                    sent: now,
                    tries: 1,
                    reauth: true,
                },
            );
            out.push(bytes);
            return None;
        }
        let why = format!("the relay refused ({code})");
        match p.kind {
            Kind::Allocate | Kind::Refresh => Some(self.fail(&why)),
            Kind::Permission(ip) => {
                tracing::info!(%ip, why, "call: a relay permission was refused");
                self.permissions.remove(&ip);
                None
            }
        }
    }

    fn schedule_refresh(&mut self, m: &Message, now: Instant) {
        let life = Duration::from_secs(u64::from(m.lifetime().unwrap_or(600)));
        self.refresh_at = Some(now + (life / 2).max(Duration::from_secs(30)));
    }

    fn fail(&mut self, why: &str) -> TurnEvent {
        self.failed = true;
        self.relayed = None;
        self.refresh_at = None;
        TurnEvent::Failed(why.to_owned())
    }

    /// Send `m` as a request (authenticated once the realm is known) and remember it.
    fn request(&mut self, kind: Kind, mut m: Message, now: Instant) -> Vec<u8> {
        let bytes = self.authed(&mut m);
        self.pending.insert(
            m.tid,
            Pending {
                kind,
                message: m,
                sent: now,
                tries: 1,
                reauth: false,
            },
        );
        bytes
    }

    /// Strip any old credentials from `m`, add the current ones and encode it.
    fn authed(&self, m: &mut Message) -> Vec<u8> {
        m.attrs
            .retain(|(k, _)| !matches!(*k, stun::USERNAME | stun::REALM | stun::NONCE));
        let (Some(realm), Some(nonce), Some(key)) = (&self.realm, &self.nonce, &self.key) else {
            return m.encode(None, false);
        };
        m.add(stun::USERNAME, self.username.as_str());
        m.add(stun::REALM, realm.as_str());
        m.add(stun::NONCE, nonce.as_str());
        m.encode(Some(key), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server that wants credentials, then allocates 198.51.100.7:50000.
    fn answer(req: &[u8], key: &[u8; 16]) -> Vec<u8> {
        let m = Message::decode(req).unwrap();
        if m.get(stun::MESSAGE_INTEGRITY).is_none() {
            let mut e = Message::new(m.method, Class::Error, m.tid);
            e.add(stun::ERROR_CODE, vec![0, 0, 4, 1]);
            e.add(stun::REALM, "relay.test");
            e.add(stun::NONCE, "n0nce");
            return e.encode(None, false);
        }
        assert!(stun::check_integrity(req, key), "the request is signed");
        assert_eq!(m.text(stun::USERNAME).as_deref(), Some("u"));
        let mut ok = Message::new(m.method, Class::Success, m.tid);
        if m.method == stun::ALLOCATE {
            ok.add_xor_addr(
                stun::XOR_RELAYED_ADDRESS,
                "198.51.100.7:50000".parse().unwrap(),
            );
            ok.add(stun::LIFETIME, 600u32.to_be_bytes().to_vec());
        }
        ok.encode(Some(key), false)
    }

    #[test]
    fn an_allocation_answers_the_credential_challenge_then_permits_and_wraps_data() {
        let key = stun::long_term_key("u", "relay.test", "p");
        let now = crate::clock::now();
        let mut c = TurnClient::new("u", "p", false);
        let mut out = Vec::new();
        let first = c.start(now);
        assert!(c.handle(&answer(&first, &key), now, &mut out).is_none());
        let second = out.pop().expect("an authenticated Allocate");
        let ev = c.handle(&answer(&second, &key), now, &mut out);
        let relayed: SocketAddr = "198.51.100.7:50000".parse().unwrap();
        assert_eq!(ev, Some(TurnEvent::Allocated { relayed }));

        let peer: SocketAddr = "203.0.113.9:4000".parse().unwrap();
        let perm = c.permit(peer.ip(), now).expect("a CreatePermission");
        let m = Message::decode(&perm).unwrap();
        assert_eq!(m.method, stun::CREATE_PERMISSION);
        assert_eq!(
            m.xor_addr(stun::XOR_PEER_ADDRESS).map(|a| a.ip()),
            Some(peer.ip())
        );
        assert!(
            c.permit(peer.ip(), now).is_none(),
            "fresh permissions are not renewed"
        );

        let sent = Message::decode(&c.send(peer, b"rtp")).unwrap();
        assert_eq!((sent.method, sent.class), (stun::SEND, Class::Indication));
        assert_eq!(sent.get(stun::DATA_ATTR), Some(&b"rtp"[..]));

        let mut data = Message::new(stun::DATA, Class::Indication, stun::tid());
        data.add_xor_addr(stun::XOR_PEER_ADDRESS, peer);
        data.add(stun::DATA_ATTR, b"back".to_vec());
        let ev = c.handle(&data.encode(None, false), now, &mut out);
        assert_eq!(
            ev,
            Some(TurnEvent::Data {
                peer,
                data: b"back".to_vec()
            })
        );
    }

    #[test]
    fn an_allocation_is_refreshed_at_half_its_lifetime_and_lost_after_four_tries() {
        let key = stun::long_term_key("u", "relay.test", "p");
        let t0 = crate::clock::now();
        let mut c = TurnClient::new("u", "p", false);
        let mut out = Vec::new();
        let first = c.start(t0);
        c.handle(&answer(&first, &key), t0, &mut out);
        let second = out.pop().unwrap();
        c.handle(&answer(&second, &key), t0, &mut out);
        assert_eq!(c.next_tick(), Some(t0 + Duration::from_secs(300)));
        c.tick(t0 + Duration::from_secs(300), &mut out);
        let refresh = Message::decode(&out.pop().unwrap()).unwrap();
        assert_eq!(refresh.method, stun::REFRESH);
        // Unanswered: resent three times, then the allocation is lost.
        let mut t = t0 + Duration::from_secs(300);
        let mut lost = None;
        for _ in 0..5 {
            t += Duration::from_secs(5);
            lost = lost.or(c.tick(t, &mut out));
        }
        assert!(matches!(lost, Some(TurnEvent::Failed(_))));
        assert!(c.failed() && c.relayed().is_none());
    }

    #[test]
    fn permissions_are_renewed_after_four_minutes_on_their_own_clock() {
        let key = stun::long_term_key("u", "relay.test", "p");
        let t0 = crate::clock::now();
        let mut c = TurnClient::new("u", "p", true);
        let mut out = Vec::new();
        let first = c.start(t0);
        c.handle(&answer(&first, &key), t0, &mut out);
        let second = out.pop().unwrap();
        c.handle(&answer(&second, &key), t0, &mut out);
        let peer: IpAddr = "203.0.113.9".parse().unwrap();
        assert!(c.permit(peer, t0).is_some());
        assert_eq!(c.next_tick(), Some(t0 + PERMISSION_RENEW));
        c.tick(t0 + PERMISSION_RENEW, &mut out);
        let renewed = out.pop().map(|b| Message::decode(&b).unwrap());
        assert_eq!(renewed.map(|m| m.method), Some(stun::CREATE_PERMISSION));
        c.abandon();
        assert!(c.failed() && c.relayed().is_none());
        assert!(c.permit(peer, t0 + PERMISSION_RENEW * 2).is_none());
    }

    #[test]
    fn a_refused_allocation_fails_with_the_code() {
        let now = crate::clock::now();
        let mut c = TurnClient::new("u", "p", true);
        let first = Message::decode(&c.start(now)).unwrap();
        let mut e = Message::new(stun::ALLOCATE, Class::Error, first.tid);
        e.add(stun::ERROR_CODE, vec![0, 0, 4, 86]);
        let ev = c.handle(&e.encode(None, false), now, &mut Vec::new());
        assert_eq!(
            ev,
            Some(TurnEvent::Failed("the relay refused (486)".into()))
        );
    }
}
