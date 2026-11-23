//! The connection's manager thread: connect, run a session, fail what was waiting, back off, repeat (Client.hs `getProtocolClient`, agent reconnect).
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, select};

use crate::smp::error::SmpError;
use crate::smp::events::{SessionInfo, SmpEventKind};
use crate::smp::transport::{Connector, Wire};

use super::backoff::Backoff;
use super::queue::{Ctl, Outgoing};
use super::shared::{Shared, lock};
use super::writer::Out;
use super::{connect, reader, writer};

pub struct Channels {
    pub cmd_rx: Receiver<Outgoing>,
    pub ctl_rx: Receiver<Ctl>,
    pub ctl_tx: Sender<Ctl>,
}

pub fn run(sh: &Arc<Shared>, connector: &dyn Connector, ch: &Channels) {
    let mut backoff = Backoff::new(sh.cfg.reconnect_min, sh.cfg.reconnect_max);
    let mut epoch = 0u64;
    while !sh.is_closed() {
        let alive = match connect::establish(sh, connector) {
            Ok(wire) => {
                epoch += 1;
                backoff.reset();
                let why = session(sh, wire, ch, epoch);
                end_session(sh, &ch.cmd_rx, &why)
            }
            Err(e) => {
                drain(sh, &ch.cmd_rx, &SmpError::NotConnected);
                !sh.is_closed() && sh.emit(SmpEventKind::ConnectFailed(e))
            }
        };
        if !alive || !pause(sh, backoff.next_delay(), ch) {
            break;
        }
    }
    sh.up.store(false, Ordering::Release);
    sh.fail_pending(&SmpError::Closed);
    drain(sh, &ch.cmd_rx, &SmpError::Closed);
    sh.close_link();
}

fn session(sh: &Arc<Shared>, wire: Wire, ch: &Channels, epoch: u64) -> SmpError {
    let Wire {
        reader: rd,
        writer: wr,
        closer,
        session: s,
    } = wire;
    *lock(&sh.closer) = Some(closer);
    sh.last_rx.store(sh.clock.now_ms(), Ordering::Release);
    let info = SessionInfo {
        version: s.version,
        session_id: s.session_id.clone(),
        server_info: s.server_info.clone(),
        block_encrypted: s.snd.encrypted(),
    };
    *lock(&sh.session) = Some(info.clone());
    sh.up.store(true, Ordering::Release);
    if !sh.emit(SmpEventKind::Up(info)) {
        return SmpError::Closed;
    }
    let (rsh, tx, version, rcv) = (Arc::clone(sh), ch.ctl_tx.clone(), s.version, s.rcv);
    let spawned = thread::Builder::new()
        .name("smp-reader".into())
        .spawn(move || {
            let err = reader::run(&rsh, rd, rcv, version);
            let _ = tx.try_send(Ctl::ReaderDone { epoch, err }); // a full queue holds a Close, which wakes the writer anyway
        });
    let handle = match spawned {
        Ok(h) => h,
        Err(e) => return e.into(),
    };
    let mut out = Out {
        writer: wr,
        snd: s.snd,
        version: s.version,
        session_id: s.session_id,
        server_key: s.server_key,
    };
    let why = writer::run(sh, &ch.cmd_rx, &ch.ctl_rx, epoch, &mut out);
    sh.close_link();
    let _ = handle.join(); // a panicked reader has already ended the session
    why
}

/// Marks the session down and fails what was waiting. False when nobody listens any more.
fn end_session(sh: &Shared, cmd_rx: &Receiver<Outgoing>, why: &SmpError) -> bool {
    sh.up.store(false, Ordering::Release);
    *lock(&sh.session) = None;
    sh.fail_pending(&SmpError::NotConnected);
    drain(sh, cmd_rx, &SmpError::NotConnected);
    !sh.is_closed() && sh.emit(SmpEventKind::Down(why.clone()))
}

fn drain(sh: &Shared, cmd_rx: &Receiver<Outgoing>, err: &SmpError) {
    for o in cmd_rx.try_iter() {
        sh.answer(&o.reply, Err(err.clone()));
    }
}

/// Waits out the back-off, refusing commands meanwhile. False when the connection was closed.
fn pause(sh: &Shared, delay: Duration, ch: &Channels) -> bool {
    let end = sh.clock.now_ms() + u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
    loop {
        let left = end.saturating_sub(sh.clock.now_ms());
        if left == 0 || sh.is_closed() {
            return !sh.is_closed();
        }
        select! {
            recv(ch.cmd_rx) -> o => if let Ok(o) = o {
                sh.answer(&o.reply, Err(SmpError::NotConnected));
            },
            recv(ch.ctl_rx) -> c => if matches!(c, Ok(Ctl::Close) | Err(_)) {
                return false;
            },
            default(Duration::from_millis(left)) => {}
        }
    }
}
