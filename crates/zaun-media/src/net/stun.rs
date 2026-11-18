//! STUN messages (RFC 5389/8489) as the TURN client needs them: encode with
//! MESSAGE-INTEGRITY and FINGERPRINT, decode, check integrity, XOR addresses, long-term keys.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use sha1::Sha1;

pub(crate) const MAGIC: u32 = 0x2112_A442;
const HEADER: usize = 20;
const FINGERPRINT_XOR: u32 = 0x5354_554e;

/// Methods.
pub(crate) const BINDING: u16 = 0x001;
pub(crate) const ALLOCATE: u16 = 0x003;
pub(crate) const REFRESH: u16 = 0x004;
pub(crate) const SEND: u16 = 0x006;
pub(crate) const DATA: u16 = 0x007;
pub(crate) const CREATE_PERMISSION: u16 = 0x008;

/// Attributes.
pub(crate) const USERNAME: u16 = 0x0006;
pub(crate) const MESSAGE_INTEGRITY: u16 = 0x0008;
pub(crate) const ERROR_CODE: u16 = 0x0009;
pub(crate) const LIFETIME: u16 = 0x000D;
pub(crate) const XOR_PEER_ADDRESS: u16 = 0x0012;
pub(crate) const DATA_ATTR: u16 = 0x0013;
pub(crate) const REALM: u16 = 0x0014;
pub(crate) const NONCE: u16 = 0x0015;
pub(crate) const XOR_RELAYED_ADDRESS: u16 = 0x0016;
pub(crate) const REQUESTED_TRANSPORT: u16 = 0x0019;
pub(crate) const XOR_MAPPED_ADDRESS: u16 = 0x0020;
#[cfg(test)]
pub(crate) const SOFTWARE: u16 = 0x8022;
pub(crate) const FINGERPRINT: u16 = 0x8028;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Request,
    Indication,
    Success,
    Error,
}

/// One STUN message: attributes in order, values without padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Message {
    pub method: u16,
    pub class: Class,
    pub tid: [u8; 12],
    pub attrs: Vec<(u16, Vec<u8>)>,
}

impl Message {
    pub fn new(method: u16, class: Class, tid: [u8; 12]) -> Self {
        Self {
            method,
            class,
            tid,
            attrs: Vec::new(),
        }
    }

    pub fn add(&mut self, kind: u16, value: impl Into<Vec<u8>>) {
        self.attrs.push((kind, value.into()));
    }

    pub fn add_xor_addr(&mut self, kind: u16, addr: SocketAddr) {
        let v = xor_addr(addr, &self.tid);
        self.add(kind, v);
    }

    pub fn get(&self, kind: u16) -> Option<&[u8]> {
        self.attrs
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| v.as_slice())
    }

    pub fn text(&self, kind: u16) -> Option<String> {
        self.get(kind)
            .map(|v| String::from_utf8_lossy(v).into_owned())
    }

    pub fn xor_addr(&self, kind: u16) -> Option<SocketAddr> {
        unxor_addr(self.get(kind)?, &self.tid)
    }

    /// The error code (e.g. 401, 438) of an error response.
    pub fn error_code(&self) -> Option<u16> {
        let v = self.get(ERROR_CODE)?;
        (v.len() >= 4).then(|| u16::from(v[2] & 0x7) * 100 + u16::from(v[3]))
    }

    pub fn lifetime(&self) -> Option<u32> {
        let v: [u8; 4] = self.get(LIFETIME)?.try_into().ok()?;
        Some(u32::from_be_bytes(v))
    }

    /// The message on the wire; MESSAGE-INTEGRITY with `key`, then FINGERPRINT if asked.
    pub fn encode(&self, key: Option<&[u8]>, fingerprint: bool) -> Vec<u8> {
        let mut b = Vec::with_capacity(128);
        b.extend_from_slice(&msg_type(self.method, self.class).to_be_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&MAGIC.to_be_bytes());
        b.extend_from_slice(&self.tid);
        for (k, v) in &self.attrs {
            put_attr(&mut b, *k, v);
        }
        if let Some(key) = key {
            let n = b.len() - HEADER + 24;
            set_len(&mut b, n);
            let mac = hmac_sha1(key, &b);
            put_attr(&mut b, MESSAGE_INTEGRITY, &mac);
        }
        if fingerprint {
            let n = b.len() - HEADER + 8;
            set_len(&mut b, n);
            let crc = crc32fast::hash(&b) ^ FINGERPRINT_XOR;
            put_attr(&mut b, FINGERPRINT, &crc.to_be_bytes());
        }
        let n = b.len() - HEADER;
        set_len(&mut b, n);
        b
    }

    pub fn decode(buf: &[u8]) -> Option<Self> {
        if !is_stun(buf) {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
        let body = buf.get(HEADER..HEADER + len)?;
        let t = u16::from_be_bytes([buf[0], buf[1]]);
        let class = match t & 0x0110 {
            0x0000 => Class::Request,
            0x0010 => Class::Indication,
            0x0100 => Class::Success,
            _ => Class::Error,
        };
        let method = (t & 0x000F) | ((t & 0x00E0) >> 1) | ((t & 0x3E00) >> 2);
        let mut tid = [0; 12];
        tid.copy_from_slice(&buf[8..20]);
        let mut attrs = Vec::new();
        let mut i = 0;
        while i + 4 <= body.len() {
            let k = u16::from_be_bytes([body[i], body[i + 1]]);
            let n = usize::from(u16::from_be_bytes([body[i + 2], body[i + 3]]));
            let v = body.get(i + 4..i + 4 + n)?;
            attrs.push((k, v.to_vec()));
            i += 4 + n.div_ceil(4) * 4;
        }
        Some(Self {
            method,
            class,
            tid,
            attrs,
        })
    }
}

/// A STUN message (not ChannelData, RTP or DTLS): the first two bits zero and the cookie.
pub(crate) fn is_stun(buf: &[u8]) -> bool {
    buf.len() >= HEADER
        && buf[0] & 0xC0 == 0
        && u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) == MAGIC
}

/// The length on the wire of the STUN message starting `buf` (for framing over TCP).
pub(crate) fn frame_len(buf: &[u8]) -> Option<usize> {
    (buf.len() >= 4).then(|| HEADER + usize::from(u16::from_be_bytes([buf[2], buf[3]])))
}

/// Whether `raw` carries a MESSAGE-INTEGRITY made with `key`.
pub(crate) fn check_integrity(raw: &[u8], key: &[u8]) -> bool {
    let Some(at) = attr_offset(raw, MESSAGE_INTEGRITY) else {
        return false;
    };
    let Some(mac) = raw.get(at + 4..at + 24) else {
        return false;
    };
    let mut head = raw[..at].to_vec();
    set_len(&mut head, at - HEADER + 24);
    hmac_sha1(key, &head).as_slice() == mac
}

/// Whether `raw` carries a correct FINGERPRINT.
#[cfg(test)]
pub(crate) fn check_fingerprint(raw: &[u8]) -> bool {
    let Some(at) = attr_offset(raw, FINGERPRINT) else {
        return false;
    };
    let Some(v) = raw.get(at + 4..at + 8) else {
        return false;
    };
    let mut head = raw[..at].to_vec();
    set_len(&mut head, at - HEADER + 8);
    (crc32fast::hash(&head) ^ FINGERPRINT_XOR).to_be_bytes() == v
}

/// The long-term credential key: MD5(username ":" realm ":" password) (RFC 5389 §15.4;
/// the strings are taken as given, without SASLprep).
pub(crate) fn long_term_key(username: &str, realm: &str, password: &str) -> [u8; 16] {
    let mut h = Md5::new();
    h.update(format!("{username}:{realm}:{password}").as_bytes());
    h.finalize().into()
}

/// A random transaction id (from the system's random source).
pub(crate) fn tid() -> [u8; 12] {
    let mut t = [0; 12];
    if rustix::rand::getrandom(&mut t, rustix::rand::GetRandomFlags::empty()).is_err() {
        // Never seen on Linux; a fixed id only makes this one transaction guessable.
        tracing::warn!("call: no random transaction id");
    }
    t
}

fn attr_offset(raw: &[u8], kind: u16) -> Option<usize> {
    let len = usize::from(u16::from_be_bytes([*raw.get(2)?, *raw.get(3)?]));
    let end = (HEADER + len).min(raw.len());
    let mut i = HEADER;
    while i + 4 <= end {
        let k = u16::from_be_bytes([raw[i], raw[i + 1]]);
        let n = usize::from(u16::from_be_bytes([raw[i + 2], raw[i + 3]]));
        if k == kind {
            return Some(i);
        }
        i += 4 + n.div_ceil(4) * 4;
    }
    None
}

fn msg_type(method: u16, class: Class) -> u16 {
    let c = match class {
        Class::Request => 0x0000,
        Class::Indication => 0x0010,
        Class::Success => 0x0100,
        Class::Error => 0x0110,
    };
    (method & 0x000F) | ((method & 0x0070) << 1) | ((method & 0x0F80) << 2) | c
}

fn put_attr(b: &mut Vec<u8>, kind: u16, v: &[u8]) {
    b.extend_from_slice(&kind.to_be_bytes());
    // Attribute values are short (an address, a name, a datagram): far below 64 KiB.
    b.extend_from_slice(&u16::try_from(v.len()).unwrap_or(u16::MAX).to_be_bytes());
    b.extend_from_slice(v);
    b.resize(b.len().div_ceil(4) * 4, 0);
}

fn set_len(b: &mut [u8], len: usize) {
    let l = u16::try_from(len).unwrap_or(u16::MAX).to_be_bytes();
    b[2] = l[0];
    b[3] = l[1];
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let Ok(mut m) = <Hmac<Sha1> as KeyInit>::new_from_slice(key) else {
        // HMAC takes keys of any length.
        return [0; 20];
    };
    m.update(data);
    m.finalize().into_bytes().into()
}

fn xor_addr(addr: SocketAddr, tid: &[u8; 12]) -> Vec<u8> {
    let port = addr.port() ^ (MAGIC >> 16) as u16;
    let mut v = Vec::with_capacity(20);
    match addr.ip() {
        IpAddr::V4(ip) => {
            v.extend_from_slice(&[0, 1]);
            v.extend_from_slice(&port.to_be_bytes());
            v.extend_from_slice(&(u32::from(ip) ^ MAGIC).to_be_bytes());
        }
        IpAddr::V6(ip) => {
            v.extend_from_slice(&[0, 2]);
            v.extend_from_slice(&port.to_be_bytes());
            let mask = xor_mask(tid);
            v.extend(ip.octets().iter().zip(mask).map(|(a, m)| a ^ m));
        }
    }
    v
}

fn unxor_addr(v: &[u8], tid: &[u8; 12]) -> Option<SocketAddr> {
    let port = u16::from_be_bytes([*v.get(2)?, *v.get(3)?]) ^ (MAGIC >> 16) as u16;
    let ip = match v.get(1)? {
        1 => {
            let a: [u8; 4] = v.get(4..8)?.try_into().ok()?;
            IpAddr::V4(Ipv4Addr::from(u32::from_be_bytes(a) ^ MAGIC))
        }
        2 => {
            let a: [u8; 16] = v.get(4..20)?.try_into().ok()?;
            let mut o = [0u8; 16];
            for ((o, a), m) in o.iter_mut().zip(a).zip(xor_mask(tid)) {
                *o = a ^ m;
            }
            IpAddr::V6(Ipv6Addr::from(o))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

fn xor_mask(tid: &[u8; 12]) -> [u8; 16] {
    let mut m = [0; 16];
    m[..4].copy_from_slice(&MAGIC.to_be_bytes());
    m[4..].copy_from_slice(tid);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    const PASSWORD: &[u8] = b"VOkJxbRl1RmTxUk/WvJxBt";

    /// RFC 5769 §2.1.
    const REQUEST: &str = "00 01 00 58 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 10 53 54 55 4e 20 74 65 73 74 20 63 6c 69 65 6e 74
        00 24 00 04 6e 00 01 ff 80 29 00 08 93 2f f9 b1 51 26 3b 36
        00 06 00 09 65 76 74 6a 3a 68 36 76 59 20 20 20
        00 08 00 14 9a ea a7 0c bf d8 cb 56 78 1e f2 b5 b2 d3 f2 49 c1 b5 71 a2
        80 28 00 04 e5 7a 3b cf";

    /// RFC 5769 §2.2.
    const RESPONSE_V4: &str = "01 01 00 3c 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 0b 74 65 73 74 20 76 65 63 74 6f 72 20
        00 20 00 08 00 01 a1 47 e1 12 a6 43
        00 08 00 14 2b 91 f5 99 fd 9e 90 c3 8c 74 89 f9 2a f9 ba 53 f0 6b e7 d7
        80 28 00 04 c0 7d 4c 96";

    /// RFC 5769 §2.3.
    const RESPONSE_V6: &str = "01 01 00 48 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 0b 74 65 73 74 20 76 65 63 74 6f 72 20
        00 20 00 14 00 02 a1 47 01 13 a9 fa a5 d3 f1 79 bc 25 f4 b5 be d2 b9 d9
        00 08 00 14 a3 82 95 4e 4b e6 7b f1 17 84 c9 7c 82 92 c2 75 bf e3 ed 41
        80 28 00 04 c8 fb 0b 4c";

    /// RFC 5769 §2.4.
    const LONG_TERM: &str = "00 01 00 60 21 12 a4 42 78 ad 34 33 c6 ad 72 c0 29 da 41 2e
        00 06 00 12 e3 83 9e e3 83 88 e3 83 aa e3 83 83 e3 82 af e3 82 b9 00 00
        00 15 00 1c 66 2f 2f 34 39 39 6b 39 35 34 64 36 4f 4c 33 34 6f 4c 39 46 53 54 76 79 36 34 73 41
        00 14 00 0b 65 78 61 6d 70 6c 65 2e 6f 72 67 00
        00 08 00 14 f6 70 24 65 6d d6 4a 3e 02 b8 e0 71 2e 85 c9 a2 8c a8 96 66";

    #[test]
    fn rfc5769_sample_request_has_a_valid_integrity_and_fingerprint() {
        let raw = hex(REQUEST);
        assert!(check_integrity(&raw, PASSWORD));
        assert!(!check_integrity(&raw, b"wrong"));
        assert!(check_fingerprint(&raw));
        let m = Message::decode(&raw).unwrap();
        assert_eq!((m.method, m.class), (BINDING, Class::Request));
        assert_eq!(m.text(USERNAME).as_deref(), Some("evtj:h6vY"));
        assert_eq!(m.text(SOFTWARE).as_deref(), Some("STUN test client"));
    }

    #[test]
    fn rfc5769_sample_responses_carry_the_mapped_addresses() {
        for (vector, addr) in [
            (RESPONSE_V4, "192.0.2.1:32853"),
            (RESPONSE_V6, "[2001:db8:1234:5678:11:2233:4455:6677]:32853"),
        ] {
            let raw = hex(vector);
            assert!(check_integrity(&raw, PASSWORD) && check_fingerprint(&raw));
            let m = Message::decode(&raw).unwrap();
            assert_eq!((m.method, m.class), (BINDING, Class::Success));
            assert_eq!(m.xor_addr(XOR_MAPPED_ADDRESS), Some(addr.parse().unwrap()));
        }
    }

    #[test]
    fn rfc5769_long_term_request_is_encoded_byte_for_byte() {
        let raw = hex(LONG_TERM);
        let mut tid = [0; 12];
        tid.copy_from_slice(&raw[8..20]);
        let mut m = Message::new(BINDING, Class::Request, tid);
        m.add(USERNAME, "マトリックス");
        m.add(NONCE, "f//499k954d6OL34oL9FSTvy64sA");
        m.add(REALM, "example.org");
        let key = long_term_key("マトリックス", "example.org", "TheMatrIX");
        assert_eq!(m.encode(Some(&key), false), raw);
        assert!(check_integrity(&raw, &key));
    }

    #[test]
    fn xor_addresses_and_message_types_round_trip() {
        let t = tid();
        for addr in ["10.1.2.3:3478", "[fe80::1]:9"] {
            let mut m = Message::new(CREATE_PERMISSION, Class::Request, t);
            m.add_xor_addr(XOR_PEER_ADDRESS, addr.parse().unwrap());
            let back = Message::decode(&m.encode(None, true)).unwrap();
            assert_eq!(
                (back.method, back.class),
                (CREATE_PERMISSION, Class::Request)
            );
            assert_eq!(back.xor_addr(XOR_PEER_ADDRESS), Some(addr.parse().unwrap()));
        }
        for (method, class) in [(DATA, Class::Indication), (ALLOCATE, Class::Error)] {
            let m = Message::new(method, class, t);
            let back = Message::decode(&m.encode(None, false)).unwrap();
            assert_eq!((back.method, back.class), (method, class));
        }
    }

    #[test]
    fn error_codes_and_framing_are_read() {
        let mut m = Message::new(ALLOCATE, Class::Error, tid());
        m.add(ERROR_CODE, vec![0, 0, 4, 38]);
        let raw = m.encode(None, false);
        assert_eq!(Message::decode(&raw).unwrap().error_code(), Some(438));
        assert_eq!(frame_len(&raw), Some(raw.len()));
        assert!(
            !is_stun(&[0x40, 0, 0, 4, 1, 2, 3, 4]),
            "ChannelData is not STUN"
        );
    }
}
