use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::certs::{Identity, identity, signed_key};
use super::support::*;
use crate::crypto::{
    Alg, KeyPair, PublicKey, dh, pad, sb_decrypt, sb_encrypt, sbc_hkdf, sbc_init, unpad,
};
use crate::encoding::{Version, VersionRange};
use crate::smp::{
    BLOCK_SIZE, HandshakeConfig, HandshakeError, Reader, ServerHello, SmpError, TlsFacts, Writer,
    client_handshake, negotiate,
};

struct Blocks(VecDeque<Vec<u8>>);
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Reader for Blocks {
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let b = self.0.pop_front().ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(&b);
        Ok(())
    }

    fn set_read_timeout(&mut self, _: Option<Duration>) -> io::Result<()> {
        Ok(())
    }
}

impl Writer for Sink {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.lock().unwrap().extend(buf);
        Ok(())
    }
}

const SESSION: [u8; 32] = [3; 32];

struct Server {
    id: Identity,
    dh: KeyPair,
}

impl Server {
    fn new() -> Self {
        Self {
            id: identity(),
            dh: keypair(Alg::X25519),
        }
    }

    fn hello(&self, session: &[u8], range: (u16, u16), info: Option<Vec<u8>>) -> Vec<u8> {
        self.hello_signed_by(&self.id.leaf, session, range, info)
    }

    fn hello_signed_by(
        &self,
        signer: &KeyPair,
        session: &[u8],
        range: (u16, u16),
        info: Option<Vec<u8>>,
    ) -> Vec<u8> {
        let h = ServerHello {
            range: VersionRange::new(Version(range.0), Version(range.1)).unwrap(),
            session_id: session.to_vec(),
            chain: self.id.chain.clone(),
            signed_key: signed_key(&self.dh.public, signer),
            server_info: info,
        };
        pad(&h.encode().unwrap(), BLOCK_SIZE).unwrap()
    }
}

fn cfg() -> HandshakeConfig {
    HandshakeConfig::standard(Duration::from_secs(1))
}

fn run(
    s: &Server,
    block: Vec<u8>,
    session: &[u8],
    key_hash: &[u8],
) -> (Result<crate::smp::Established, SmpError>, Vec<u8>) {
    let out = Arc::new(Mutex::new(Vec::new()));
    let facts = TlsFacts {
        tls_unique: session,
        peer_chain: &s.id.chain,
    };
    let r = client_handshake(
        &mut Blocks([block].into()),
        &mut Sink(Arc::clone(&out)),
        &facts,
        key_hash,
        &cfg(),
    );
    let written = out.lock().unwrap().clone();
    (r, written)
}

#[test]
fn handshake_negotiates_the_highest_version_and_sends_our_dh_key() {
    let s = Server::new();
    let (est, written) = run(
        &s,
        s.hello(&SESSION, (15, 23), None),
        &SESSION,
        &s.id.key_hash,
    );
    let est = est.unwrap();
    assert_eq!(est.version, 23);
    assert_eq!(est.server_key, s.dh.public);
    assert!(est.snd.encrypted());
    assert_eq!(written.len(), BLOCK_SIZE);
    let hello = unpad(&written).unwrap();
    assert_eq!(&hello[..2], &[0, 23]);
    assert_eq!(&hello[2..35], &[&[32u8][..], &s.id.key_hash].concat()[..]);
    assert_eq!(
        hello[35], 44,
        "the client DH key follows the key hash as a ByteString"
    );
    assert_eq!(
        &hello[hello.len() - 2..],
        b"F0",
        "proxy flag F, then '0' for no client service"
    );
}

#[test]
fn block_encryption_chains_run_in_opposite_directions_on_each_side() {
    let s = Server::new();
    let (est, written) = run(
        &s,
        s.hello(&SESSION, (15, 23), None),
        &SESSION,
        &s.id.key_hash,
    );
    let mut est = est.unwrap();
    let client_pub = PublicKey::from_der(&unpad(&written).unwrap()[36..80]).unwrap();
    let secret = dh(&client_pub, &s.dh.private).unwrap();
    let (server_snd, server_rcv) = sbc_init(&SESSION, secret.as_bytes()).unwrap();

    let block = est.snd.seal(b"from client").unwrap();
    assert_eq!(block.len(), BLOCK_SIZE);
    let (k, n, _) = sbc_hkdf(&server_rcv).unwrap();
    assert_eq!(sb_decrypt(&k, &n, &block).unwrap(), b"from client");

    let (k, n, _) = sbc_hkdf(&server_snd).unwrap();
    let reply = sb_encrypt(&k, &n, b"from server", BLOCK_SIZE - 16).unwrap();
    assert_eq!(est.rcv.open(&reply).unwrap(), b"from server");
}

#[test]
fn each_block_uses_the_next_chain_key() {
    let s = Server::new();
    let (est, _) = run(
        &s,
        s.hello(&SESSION, (15, 23), None),
        &SESSION,
        &s.id.key_hash,
    );
    let mut est = est.unwrap();
    let (a, b) = (est.snd.seal(b"x").unwrap(), est.snd.seal(b"x").unwrap());
    assert_ne!(a, b);
}

#[test]
fn server_info_is_read_at_v21_and_later() {
    let s = Server::new();
    let (est, _) = run(
        &s,
        s.hello(&SESSION, (15, 23), Some(b"{\"a\":1}".to_vec())),
        &SESSION,
        &s.id.key_hash,
    );
    assert_eq!(est.unwrap().server_info.as_deref(), Some(&b"{\"a\":1}"[..]));
    let (est, _) = run(
        &s,
        s.hello(&SESSION, (15, 20), None),
        &SESSION,
        &s.id.key_hash,
    );
    assert_eq!(est.unwrap().version, 20);
}

#[test]
fn a_session_id_that_differs_from_the_tls_binding_is_refused() {
    let s = Server::new();
    let (r, _) = run(
        &s,
        s.hello(&[4; 32], (15, 23), None),
        &SESSION,
        &s.id.key_hash,
    );
    assert_eq!(r.err(), Some(SmpError::Handshake(HandshakeError::Session)));
}

#[test]
fn a_key_hash_other_than_the_address_is_refused() {
    let s = Server::new();
    let (r, _) = run(&s, s.hello(&SESSION, (15, 23), None), &SESSION, &[0; 32]);
    assert_eq!(r.err(), Some(SmpError::Handshake(HandshakeError::Identity)));
}

#[test]
fn a_dh_key_signed_by_someone_else_is_refused() {
    let s = Server::new();
    let block = s.hello_signed_by(&keypair(Alg::Ed25519), &SESSION, (15, 23), None);
    let (r, _) = run(&s, block, &SESSION, &s.id.key_hash);
    assert_eq!(r.err(), Some(SmpError::Handshake(HandshakeError::BadAuth)));
}

#[test]
fn servers_below_our_floor_have_no_common_version() {
    let s = Server::new();
    let (r, _) = run(
        &s,
        s.hello(&SESSION, (6, 14), None),
        &SESSION,
        &s.id.key_hash,
    );
    assert_eq!(r.err(), Some(SmpError::Handshake(HandshakeError::Version)));
    let theirs = VersionRange::new(Version(6), Version(14)).unwrap();
    assert_eq!(
        negotiate(&theirs, &cfg().range),
        Err(HandshakeError::Version)
    );
}

#[test]
fn encryption_can_be_left_off_for_a_plain_session() {
    let s = Server::new();
    let out = Arc::new(Mutex::new(Vec::new()));
    let facts = TlsFacts {
        tls_unique: &SESSION,
        peer_chain: &s.id.chain,
    };
    let c = HandshakeConfig {
        block_encryption: false,
        ..cfg()
    };
    let r = client_handshake(
        &mut Blocks([s.hello(&SESSION, (15, 23), None)].into()),
        &mut Sink(Arc::clone(&out)),
        &facts,
        &s.id.key_hash,
        &c,
    );
    let mut est = r.unwrap();
    assert!(!est.snd.encrypted());
    assert_eq!(unpad(&est.snd.seal(b"hi").unwrap()).unwrap(), b"hi");
    let hello = out.lock().unwrap().clone();
    assert_eq!(&unpad(&hello).unwrap()[35..], b"F0");
}
