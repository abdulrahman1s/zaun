//! State the connection's threads share.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use std::time::Duration;

use crossbeam_channel::{SendTimeoutError, Sender};

use crate::smp::address::SmpServer;
use crate::smp::clock::Clock;
use crate::smp::config::ConnConfig;
use crate::smp::error::SmpError;
use crate::smp::events::{SessionInfo, SmpEvent, SmpEventKind};
use crate::smp::responses::Response;
use crate::smp::transport::Closer;

use super::pending::{Pending, Reply};

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct Shared {
    pub server: Arc<SmpServer>,
    pub cfg: ConnConfig,
    pub clock: Arc<dyn Clock>,
    pub pending: Mutex<Pending>,
    pub session: Mutex<Option<SessionInfo>>,
    pub up: AtomicBool,
    pub closed: AtomicBool,
    pub closer: Mutex<Option<Arc<dyn Closer>>>,
    /// Clock time of the last block received.
    pub last_rx: AtomicU64,
    pub events: Sender<SmpEvent>,
}

impl Shared {
    /// Waits for room in the event channel, but not past `close`. False when the connection is closed or the owner dropped the receiver.
    pub fn emit(&self, kind: SmpEventKind) -> bool {
        let mut event = SmpEvent {
            server: Arc::clone(&self.server),
            kind,
        };
        loop {
            match self.events.send_timeout(event, Duration::from_millis(100)) {
                Ok(()) => return true,
                Err(SendTimeoutError::Timeout(e)) if !self.is_closed() => event = e,
                Err(SendTimeoutError::Timeout(_) | SendTimeoutError::Disconnected(_)) => {
                    return false;
                }
            }
        }
    }

    /// Delivers a command's answer. A tagged answer waits for room like any event.
    pub fn answer(&self, reply: &Reply, answer: Result<Response, SmpError>) {
        match reply {
            // bounded(1) and answered once, so this never blocks; the caller may have stopped waiting
            Reply::Channel(tx) => drop(tx.try_send(answer)),
            Reply::Tagged(tag) => drop(self.emit(SmpEventKind::Reply { tag: *tag, answer })),
        }
    }

    /// Fails every command waiting for an answer, as when the session ends.
    pub fn fail_pending(&self, err: &SmpError) {
        let gone = lock(&self.pending).take_all();
        for e in gone {
            self.answer(&e.reply, Err(err.clone()));
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn close_link(&self) {
        if let Some(c) = lock(&self.closer).take() {
            c.close();
        }
    }
}
