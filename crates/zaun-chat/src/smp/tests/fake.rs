//! An in-memory SMP server end: blocks travel over channels, the session is already "handshaken" and plain.
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};

use super::support::keypair;
use crate::crypto::{Alg, KeyPair, pad, unpad};
use crate::smp::{
    BLOCK_SIZE, Closer, ConnConfig, Connector, Established, Incoming, RcvBlocks, Reader, Response,
    SmpError, SmpServer, SndBlocks, Wire, Writer, encode_transmission, pack, parse_payload,
};

struct PipeReader {
    rx: Receiver<Vec<u8>>,
    closed: Arc<AtomicBool>,
}

struct PipeWriter {
    tx: Sender<Vec<u8>>,
    closed: Arc<AtomicBool>,
}

struct PipeCloser(Arc<AtomicBool>);

impl Reader for PipeReader {
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        loop {
            if self.closed.load(Ordering::Acquire) {
                return Err(io::ErrorKind::ConnectionReset.into());
            }
            match self.rx.recv_timeout(Duration::from_millis(5)) {
                Ok(b) => {
                    buf.copy_from_slice(&b);
                    return Ok(());
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
            }
        }
    }

    fn set_read_timeout(&mut self, _: Option<Duration>) -> io::Result<()> {
        Ok(())
    }
}

impl Writer for PipeWriter {
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.tx
            .send(buf.to_vec())
            .map_err(|_| io::ErrorKind::BrokenPipe.into())
    }
}

impl Closer for PipeCloser {
    fn close(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// The server's side of one connection.
pub struct Peer {
    pub version: u16,
    pub session_id: Vec<u8>,
    pub dh: KeyPair,
    to_client: Sender<Vec<u8>>,
    from_client: Receiver<Vec<u8>>,
    closed: Arc<AtomicBool>,
}

impl Peer {
    /// The next block from the client, parsed; `None` after `wait`.
    pub fn recv(&self, wait: Duration) -> Option<Vec<Incoming>> {
        let block = self.from_client.recv_timeout(wait).ok()?;
        Some(parse_payload(self.version, unpad(&block).unwrap()).unwrap())
    }

    /// Everything the client sends within `wait`, flattened.
    pub fn recv_all(&self, wait: Duration) -> Vec<Incoming> {
        std::iter::from_fn(|| self.recv(wait)).flatten().collect()
    }

    /// One block holding all of `items` (corr id, entity id, response); an empty corr id is a push.
    pub fn send(&self, items: &[(&[u8], &[u8], Response)]) {
        let ts: Vec<Vec<u8>> = items
            .iter()
            .map(|(c, e, r)| {
                encode_transmission(self.version, &[], c, e, &r.encode(self.version).unwrap())
                    .unwrap()
            })
            .collect();
        let (payloads, _) = pack(&ts, BLOCK_SIZE - 2);
        for p in payloads {
            let _ = self.to_client.send(pad(&p, BLOCK_SIZE).unwrap()); // the client may be gone
        }
    }

    pub fn reply(&self, to: &Incoming, r: Response) {
        self.send(&[(&to.corr_id, &to.entity_id, r)]);
    }

    pub fn push(&self, entity: &[u8], r: Response) {
        self.send(&[(&[], entity, r)]);
    }

    /// Cuts the connection as a dying network would.
    pub fn drop_link(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

/// Hands the test a `Peer` for every connection the client makes; refuses the first `refuse` attempts.
pub struct FakeConnector {
    peers: Sender<Peer>,
    pub version: u16,
    refuse: Mutex<usize>,
}

impl FakeConnector {
    pub fn new(version: u16) -> (Arc<Self>, Receiver<Peer>) {
        let (tx, rx) = unbounded();
        (
            Arc::new(Self {
                peers: tx,
                version,
                refuse: Mutex::new(0),
            }),
            rx,
        )
    }

    pub fn refuse_next(&self, n: usize) {
        *self.refuse.lock().unwrap() = n;
    }
}

impl Connector for FakeConnector {
    fn connect(
        &self,
        _: &SmpServer,
        _: &str,
        _: &ConnConfig,
        slot: &dyn Fn(Arc<dyn Closer>),
    ) -> Result<Wire, SmpError> {
        {
            let mut refuse = self.refuse.lock().unwrap();
            if *refuse > 0 {
                *refuse -= 1;
                return Err(SmpError::Io("refused".into()));
            }
        }
        let (to_client, client_rx) = bounded(64);
        let (client_tx, from_client) = bounded(64);
        let closed = Arc::new(AtomicBool::new(false));
        let dh = keypair(Alg::X25519);
        let session_id = vec![7u8; 32];
        let closer: Arc<dyn Closer> = Arc::new(PipeCloser(Arc::clone(&closed)));
        slot(Arc::clone(&closer));
        let session = Established {
            version: self.version,
            session_id: session_id.clone(),
            server_key: dh.public.clone(),
            server_info: None,
            snd: SndBlocks::plain(),
            rcv: RcvBlocks::plain(),
        };
        let _ = self.peers.send(Peer {
            version: self.version,
            session_id,
            dh,
            to_client,
            from_client,
            closed: Arc::clone(&closed),
        });
        Ok(Wire {
            reader: Box::new(PipeReader {
                rx: client_rx,
                closed: Arc::clone(&closed),
            }),
            writer: Box::new(PipeWriter {
                tx: client_tx,
                closed,
            }),
            closer,
            session,
        })
    }
}
