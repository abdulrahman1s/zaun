//! One agent with its store and event channel, and a pump that runs several of them in the test thread as the engine loop would.
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, bounded};

use crate::agent::client::{AgentClient, AgentConfig, AgentEvent, WallClock};
use crate::smp::{Clock, Connector, SmpEvent, SystemClock};
use crate::store::Store;

/// Wall time the test moves by hand.
pub struct TestClock(pub AtomicI64);

impl WallClock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::Acquire)
    }
}

impl TestClock {
    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::AcqRel);
    }
}

/// A scratch folder removed on drop.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Self {
        let name: String = crate::crypto::random_vec(8)
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let dir = std::env::temp_dir().join(format!("zaun-agent-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Node {
    pub agent: AgentClient,
    pub store: Store,
    pub rx: Receiver<SmpEvent>,
    pub events: Vec<AgentEvent>,
    pub clock: Arc<dyn WallClock>,
}

impl Node {
    pub fn open(
        dir: &Dir,
        name: &str,
        cfg: AgentConfig,
        connector: Arc<dyn Connector>,
        clock: Arc<dyn WallClock>,
    ) -> Self {
        let store = Store::open(&dir.0.join(format!("{name}.db")), "test key").unwrap();
        let (tx, rx) = bounded(1024);
        let smp_clock = Arc::new(SystemClock::new());
        let agent = AgentClient::with_parts(cfg, tx, connector, smp_clock, Arc::clone(&clock));
        let mut n = Self {
            agent,
            store,
            rx,
            events: Vec::new(),
            clock,
        };
        n.agent.subscribe_all(&mut n.store).unwrap();
        n.collect();
        n
    }

    /// Handles what has arrived; true when anything did.
    pub fn step(&mut self, wait: Duration) -> bool {
        let mut any = false;
        if let Ok(ev) = self.rx.recv_timeout(wait) {
            self.agent.on_smp(&mut self.store, ev);
            any = true;
        }
        while let Ok(ev) = self.rx.try_recv() {
            self.agent.on_smp(&mut self.store, ev);
        }
        let now = self.clock.now_ms();
        self.agent.on_tick(&mut self.store, now);
        self.collect();
        any
    }

    pub fn collect(&mut self) {
        self.events.extend(self.agent.drain_events());
    }

    /// Removes and returns the first event `f` picks.
    pub fn take<T>(&mut self, f: impl Fn(&AgentEvent) -> Option<T>) -> Option<T> {
        let i = self.events.iter().position(|e| f(e).is_some())?;
        f(&self.events.remove(i))
    }
}

/// Runs the nodes until `done` finds what it waits for, or panics after `limit`.
pub fn pump<T>(
    nodes: &mut [&mut Node],
    limit: Duration,
    mut done: impl FnMut(&mut [&mut Node]) -> Option<T>,
) -> T {
    let clock = SystemClock::new();
    let limit = u64::try_from(limit.as_millis()).unwrap();
    loop {
        if let Some(t) = done(nodes) {
            return t;
        }
        let events: Vec<_> = nodes.iter().map(|n| &n.events).collect();
        assert!(clock.now_ms() < limit, "timed out; events: {events:?}");
        for n in nodes.iter_mut() {
            n.step(Duration::from_millis(2));
        }
    }
}
