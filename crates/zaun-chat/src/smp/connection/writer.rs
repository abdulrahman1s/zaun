//! The writer thread of one session: sends queued commands in batches, expires waiting ones, pings, and ends the session on silence.
use std::sync::atomic::Ordering;
use std::time::Duration;

use crossbeam_channel::{Receiver, select};

use crate::crypto::{PublicKey, random_vec};
use crate::smp::auth::Authorizer;
use crate::smp::block::{SndBlocks, max_payload};
use crate::smp::commands::Command;
use crate::smp::error::SmpError;
use crate::smp::transmission::{encode_transmission, pack};
use crate::smp::transport::Writer;

use super::liveness::{Liveness, Verdict};
use super::pending::{Entry, Reply};
use super::queue::{Ctl, Outgoing, Request};
use super::shared::{Shared, lock};

/// Commands taken from the queue for one round of batching.
const DRAIN: usize = 64;

pub struct Out {
    pub writer: Box<dyn Writer>,
    pub snd: SndBlocks,
    pub version: u16,
    pub session_id: Vec<u8>,
    pub server_key: PublicKey,
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Runs until the session must end and returns why.
pub fn run(
    sh: &Shared,
    cmd_rx: &Receiver<Outgoing>,
    ctl_rx: &Receiver<Ctl>,
    epoch: u64,
    out: &mut Out,
) -> SmpError {
    let ping_every = ms(sh.cfg.ping_interval);
    let start = sh.clock.now_ms();
    let mut live = Liveness::new(sh.cfg.max_timeouts, ms(sh.cfg.late_after), start);
    let mut next_ping = start + ping_every;
    loop {
        if sh.is_closed() {
            return SmpError::Closed;
        }
        let now = sh.clock.now_ms();
        live.observe_rx(sh.last_rx.load(Ordering::Acquire));
        let mut probe = false;
        let expired = lock(&sh.pending).expire(now);
        for e in expired {
            sh.answer(&e.reply, Err(SmpError::Timeout));
            match live.on_timeout(e.sent_at, e.deadline, now) {
                Verdict::Dead => return SmpError::Unresponsive,
                Verdict::Probe => probe = true,
                Verdict::Alive => {}
            }
        }
        let ping_due = ping_every > 0 && now >= next_ping;
        if (ping_due || probe)
            && !lock(&sh.pending).has_ping()
            && let Err(e) = send(sh, out, vec![ping()])
        {
            return e;
        }
        if ping_due {
            next_ping = now + ping_every;
        }
        let next = [
            lock(&sh.pending).next_deadline(),
            (ping_every > 0).then_some(next_ping),
        ]
        .into_iter()
        .flatten()
        .min();
        let wait = next.map_or(Duration::from_secs(3600), |t| {
            Duration::from_millis(t.saturating_sub(now).max(1))
        });
        select! {
            recv(cmd_rx) -> first => {
                let Ok(first) = first else { return SmpError::Closed };
                let mut batch = vec![to_prepared(first)];
                batch.extend(cmd_rx.try_iter().take(DRAIN - 1).map(to_prepared));
                if let Err(e) = send(sh, out, batch) {
                    return e;
                }
            }
            recv(ctl_rx) -> c => match c {
                Ok(Ctl::ReaderDone { epoch: g, err }) if g == epoch => return err,
                Ok(Ctl::ReaderDone { .. }) => {}
                Ok(Ctl::Close) | Err(_) => return SmpError::Closed,
            },
            default(wait) => {}
        }
    }
}

struct Prepared {
    request: Request,
    reply: Reply,
    is_ping: bool,
}

fn to_prepared(o: Outgoing) -> Prepared {
    Prepared {
        request: o.request,
        reply: o.reply,
        is_ping: false,
    }
}

fn ping() -> Prepared {
    let (reply, _rx) = crossbeam_channel::bounded(1); // nobody waits for a keepalive answer; only its arrival counts
    Prepared {
        request: Request::new(Command::Ping, Vec::new(), None),
        reply: Reply::Channel(reply),
        is_ping: true,
    }
}

/// Authorises, registers and writes `batch`; per-command failures go to that command's caller, a write failure ends the session.
fn send(sh: &Shared, out: &mut Out, batch: Vec<Prepared>) -> Result<(), SmpError> {
    let (mut ts, mut corrs) = (Vec::new(), Vec::new());
    for p in batch {
        match prepare(sh, out, &p) {
            Ok((corr, t)) => {
                corrs.push(corr);
                ts.push(t);
            }
            Err(e) => sh.answer(&p.reply, Err(e)),
        }
    }
    let (payloads, too_large) = pack(&ts, max_payload(out.snd.encrypted()));
    for i in too_large {
        let entry = corrs.get(i).and_then(|c| lock(&sh.pending).take(c));
        if let Some(e) = entry {
            sh.answer(&e.reply, Err(SmpError::LargeMsg));
        }
    }
    payloads
        .iter()
        .try_for_each(|p| out.snd.write(&mut *out.writer, p))
}

fn prepare(sh: &Shared, out: &Out, p: &Prepared) -> Result<(Vec<u8>, Vec<u8>), SmpError> {
    let r = &p.request;
    let corr = match &r.corr_id {
        Some(c) => c.clone(),
        None => random_vec(24)?,
    };
    let command = r.command.encode(out.version)?;
    let auth = Authorizer {
        session_id: &out.session_id,
        server_key: &out.server_key,
    };
    let authenticator = auth.authenticate(r.auth_key.as_ref(), &corr, &r.entity_id, &command)?;
    let t = encode_transmission(out.version, &authenticator, &corr, &r.entity_id, &command)?;
    let now = sh.clock.now_ms();
    let entry = Entry {
        reply: p.reply.clone(),
        entity_id: r.entity_id.clone(),
        sent_at: now,
        deadline: now + ms(sh.cfg.command_timeout),
        is_ping: p.is_ping,
    };
    lock(&sh.pending).insert(corr.clone(), entry)?;
    Ok((corr, t))
}
