//! The in-memory server as an smp `Connector`: each connect is a session with its own server thread.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};

use super::pipe::{PipeCloser, PipeReader, PipeWriter};
use super::queues::Push;
use super::server::{FakeSmp, Session};
use crate::crypto::{pad, random_vec, unpad};
use crate::smp::{
    BLOCK_SIZE, Closer, ConnConfig, Connector, Established, RcvBlocks, Response, SmpError,
    SmpServer, SndBlocks, Wire, encode_transmission, pack, parse_payload,
};

pub struct FakeConnector(pub Arc<FakeSmp>);

impl Connector for FakeConnector {
    fn connect(
        &self,
        _: &SmpServer,
        _: &str,
        _: &ConnConfig,
        slot: &dyn Fn(Arc<dyn Closer>),
    ) -> Result<Wire, SmpError> {
        let (to_client, client_rx) = unbounded();
        let (client_tx, from_client) = unbounded();
        let closed = Arc::new(AtomicBool::new(false));
        let session_id = random_vec(32).unwrap();
        let (sid, server_key) = {
            let mut i = self.0.lock();
            if i.refuse {
                return Err(SmpError::Io("refused".into()));
            }
            i.next_session += 1;
            let sid = i.next_session;
            let s = Session {
                tx: to_client,
                closed: Arc::clone(&closed),
                session_id: session_id.clone(),
            };
            i.sessions.insert(sid, s);
            (sid, i.server_key.public.clone())
        };
        let closer: Arc<dyn Closer> = Arc::new(PipeCloser(Arc::clone(&closed)));
        slot(Arc::clone(&closer));
        let (srv, flag) = (Arc::clone(&self.0), Arc::clone(&closed));
        thread::spawn(move || serve(&srv, sid, &from_client, &flag));
        let session = Established {
            version: self.0.version,
            session_id,
            server_key,
            server_info: None,
            snd: SndBlocks::plain(),
            rcv: RcvBlocks::plain(),
        };
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

fn serve(srv: &FakeSmp, sid: u64, from_client: &Receiver<Vec<u8>>, closed: &AtomicBool) {
    let v = srv.version;
    while !closed.load(Ordering::Acquire) {
        let block = match from_client.recv_timeout(Duration::from_millis(5)) {
            Ok(b) => b,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let Ok(ts) = parse_payload(v, unpad(&block).unwrap()) else {
            break;
        };
        let mut pushes: Vec<Push> = Vec::new();
        let mut out = Vec::new();
        let txs = {
            let mut i = srv.lock();
            for t in &ts {
                let r = i.handle(v, sid, t, &mut pushes);
                out.push((sid, t.corr_id.clone(), t.entity_id.clone(), r));
            }
            out.extend(pushes.into_iter().map(|(s, e, r)| (s, Vec::new(), e, r)));
            let txs: Vec<Option<Sender<Vec<u8>>>> = out
                .iter()
                .map(|(s, ..)| i.sessions.get(s).map(|x| x.tx.clone()))
                .collect();
            txs
        };
        for ((_, corr, entity, r), tx) in out.into_iter().zip(txs) {
            if let Some(tx) = tx {
                write(&tx, v, &corr, &entity, &r);
            }
        }
    }
    srv.lock().end_session(sid);
}

fn write(tx: &Sender<Vec<u8>>, v: u16, corr: &[u8], entity: &[u8], r: &Response) {
    let t = encode_transmission(v, &[], corr, entity, &r.encode(v).unwrap()).unwrap();
    let (payloads, _) = pack(&[t], BLOCK_SIZE - 2);
    for p in payloads {
        let _ = tx.send(pad(&p, BLOCK_SIZE).unwrap()); // the client may be gone
    }
}
