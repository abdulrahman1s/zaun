//! An in-memory SMP server for the agent tests: queues, subscriptions and checked command authorisation, over plain blocks.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crossbeam_channel::Sender;

use super::queues::Queues;
use crate::crypto::{Alg, KeyPair};

pub struct Session {
    pub tx: Sender<Vec<u8>>,
    pub closed: Arc<AtomicBool>,
    pub session_id: Vec<u8>,
}

pub struct Inner {
    pub queues: Queues,
    pub sessions: HashMap<u64, Session>,
    pub next_session: u64,
    pub refuse: bool,
    pub server_key: KeyPair,
    /// Command names in arrival order, for assertions.
    pub log: Vec<&'static str>,
}

pub struct FakeSmp {
    pub version: u16,
    pub inner: Mutex<Inner>,
}

impl FakeSmp {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            version: 23,
            inner: Mutex::new(Inner {
                queues: Queues {
                    quota: 128,
                    ..Queues::default()
                },
                sessions: HashMap::new(),
                next_session: 0,
                refuse: false,
                server_key: KeyPair::generate(Alg::X25519).unwrap(),
                log: Vec::new(),
            }),
        })
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap()
    }

    /// Cuts every connection, as a network failure would; with `refuse`, new ones fail until `refuse(false)`.
    pub fn drop_links(&self) {
        let mut i = self.lock();
        let ids: Vec<u64> = i.sessions.keys().copied().collect();
        for id in ids {
            i.end_session(id);
        }
    }

    pub fn refuse(&self, on: bool) {
        self.lock().refuse = on;
    }

    pub fn count(&self, name: &str) -> usize {
        self.lock().log.iter().filter(|n| **n == name).count()
    }
}

impl Inner {
    pub fn end_session(&mut self, id: u64) {
        if let Some(s) = self.sessions.remove(&id) {
            s.closed.store(true, Ordering::Release);
        }
        for q in self.queues.by_rcv.values_mut() {
            if q.subscriber == Some(id) {
                q.subscriber = None;
                q.delivered = None;
            }
        }
    }
}
