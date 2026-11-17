//! The service without a thread, for the headless CLI and tests (ARCHITECTURE §5).

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::app_service::{AppService, Msg, ServiceDeps};
use super::event::Event;
use super::session_watch;
use super::snapshot::SnapshotHub;
use crate::error::Result;
use crate::ids::SessionId;
use crate::library::Library;

impl AppService {
    /// Start-up runs; then the caller drives [`AppService::handle`] and [`AppService::pump`].
    /// No watcher.
    pub fn open_headless(mut deps: ServiceDeps) -> Result<(Self, Receiver<Event>)> {
        deps.watch_sessions = false;
        let (tx, rx) = channel::<Msg>();
        let (ev_tx, ev_rx) = channel::<Event>();
        let mut svc = Self::open(deps, tx, ev_tx, Arc::new(Mutex::new(None)))?;
        svc.headless_rx = Some(rx);
        svc.after_start();
        Ok((svc, ev_rx))
    }

    /// Handle the next message (a job completion), waiting at most `timeout`. Returns
    /// whether one was handled.
    pub fn pump(&mut self, timeout: Duration) -> bool {
        let Some(rx) = self.headless_rx.take() else {
            return false;
        };
        let got = match rx.recv_timeout(timeout) {
            Ok(Msg::Internal(i)) => {
                self.handle_internal(i);
                true
            }
            Ok(Msg::Cmd(c)) => {
                self.handle(c);
                true
            }
            Err(_) => false,
        };
        self.headless_rx = Some(rx);
        got
    }

    /// Feed a watcher poll by hand (tests; the CLI waits through the database).
    pub fn poll_sessions_now(&mut self) {
        let mut tracked: BTreeSet<SessionId> = self.launches.keys().copied().collect();
        if let Some(p) = session_watch::poll(self.lib.db_mut(), &mut tracked) {
            self.on_sessions(p);
        }
    }

    pub fn hub(&self) -> &Arc<SnapshotHub> {
        &self.hub
    }

    /// Read access to the library.
    pub fn library(&mut self) -> &mut Library {
        &mut self.lib
    }
}
