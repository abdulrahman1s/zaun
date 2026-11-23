//! One SMP server connection: a manager/writer thread and a reader thread, reconnecting with back-off (ADR-0012; nook smp/client.ts).
//!
//! `send` never blocks; a command's answer arrives on the returned channel, or with `send_tagged` as an `SmpEventKind::Reply`
//! on the event channel, so an owner thread can wait on one channel. Pushes and up/down changes go to the event channel,
//! and the owner re-sends its SUBs on `Up`. Authorisation is computed in the writer, against the session the command goes out on.
mod backoff;
mod connect;
mod liveness;
mod pending;
mod queue;
mod reader;
mod run;
mod shared;
mod writer;

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};

use super::address::SmpServer;
use super::clock::{Clock, SystemClock};
use super::config::ConnConfig;
use super::error::SmpError;
use super::events::{SessionInfo, SmpEvent};
use super::responses::Response;
use super::tls_connector::TlsConnector;
use super::transport::Connector;
use pending::{Pending, Reply};
use queue::{Ctl, Outgoing};
use run::Channels;
use shared::{Shared, lock};

pub use queue::Request;

/// The answer to one command: the response, or the server's ERR, a timeout, or the connection going down.
pub type Answer = Result<Response, SmpError>;

pub struct SmpConnection {
    shared: Arc<Shared>,
    cmd_tx: Sender<Outgoing>,
    ctl_tx: Sender<Ctl>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl SmpConnection {
    /// Starts connecting in the background over TCP and TLS. Returns at once; `SmpEventKind::Up` says when it is ready.
    pub fn open(
        server: SmpServer,
        cfg: ConnConfig,
        events: Sender<SmpEvent>,
    ) -> Result<Self, SmpError> {
        Self::open_with(
            server,
            cfg,
            Arc::new(TlsConnector),
            Arc::new(SystemClock::new()),
            events,
        )
    }

    pub fn open_with(
        server: SmpServer,
        cfg: ConnConfig,
        connector: Arc<dyn Connector>,
        clock: Arc<dyn Clock>,
        events: Sender<SmpEvent>,
    ) -> Result<Self, SmpError> {
        let (cmd_tx, cmd_rx) = bounded(cfg.queue_capacity.max(1));
        let (ctl_tx, ctl_rx) = bounded(8);
        let shared = Arc::new(Shared {
            server: Arc::new(server),
            pending: Mutex::new(Pending::new(cfg.max_pending)),
            cfg,
            clock,
            session: Mutex::new(None),
            up: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            closer: Mutex::new(None),
            last_rx: AtomicU64::new(0),
            events,
        });
        let sh = Arc::clone(&shared);
        let channels = Channels {
            cmd_rx,
            ctl_rx,
            ctl_tx: ctl_tx.clone(),
        };
        let handle = thread::Builder::new()
            .name("smp-conn".into())
            .spawn(move || run::run(&sh, &*connector, &channels))?;
        Ok(Self {
            shared,
            cmd_tx,
            ctl_tx,
            thread: Mutex::new(Some(handle)),
        })
    }

    /// Queues `request`. Fails at once when the connection is closed, down (`NotConnected`) or its queue is full (`Busy`).
    pub fn send(&self, request: Request) -> Result<Receiver<Answer>, SmpError> {
        let (reply, answer) = bounded(1);
        self.enqueue(request, Reply::Channel(reply))?;
        Ok(answer)
    }

    /// Like `send`, but the answer arrives on the event channel as `SmpEventKind::Reply { tag, .. }`. Every request
    /// queued here is answered exactly once there (with an error when the session ends), unless the connection is closed.
    pub fn send_tagged(&self, request: Request, tag: u64) -> Result<(), SmpError> {
        self.enqueue(request, Reply::Tagged(tag))
    }

    fn enqueue(&self, request: Request, reply: Reply) -> Result<(), SmpError> {
        if self.shared.is_closed() {
            return Err(SmpError::Closed);
        }
        if !self.is_up() {
            return Err(SmpError::NotConnected);
        }
        match self.cmd_tx.try_send(Outgoing { request, reply }) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(SmpError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(SmpError::Closed),
        }
    }

    /// `send` and wait. For tests and tools; the engine thread uses `send` and collects answers itself.
    pub fn call(&self, request: Request) -> Answer {
        self.send(request)?.recv().unwrap_or(Err(SmpError::Closed))
    }

    #[must_use]
    pub fn is_up(&self) -> bool {
        self.shared.up.load(std::sync::atomic::Ordering::Acquire)
    }

    #[must_use]
    pub fn session(&self) -> Option<SessionInfo> {
        lock(&self.shared.session).clone()
    }

    #[must_use]
    pub fn server(&self) -> &Arc<SmpServer> {
        &self.shared.server
    }

    /// Stops the threads, fails what is waiting and closes the socket. Safe to call twice.
    pub fn close(&self) {
        self.shared
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        let _ = self.ctl_tx.try_send(Ctl::Close); // a full queue is being drained by a thread that checks `closed`
        self.shared.close_link();
        if let Some(h) = lock(&self.thread).take() {
            let _ = h.join(); // a panicked manager has nothing left to clean up
        }
    }
}

impl Drop for SmpConnection {
    fn drop(&mut self) {
        self.close();
    }
}
