//! The session watcher (PROC-011): the GUI's view of sessions owned by supervisors.
//!
//! One thread with its own read-only connection. While any session is tracked or open it
//! polls once per second: the open rows plus the rows of tracked sessions (to see them end).
//! It reports a poll only when something the UI shows changed (state, end, process IDs).
//! With nothing to watch it blocks on its channel: **zero wake-ups when idle**
//! (PERFORMANCE §8). It never writes; liveness checks and recovery happen on the AppService
//! thread, which owns the read-write connection.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::ids::SessionId;
use crate::model::{Session, SessionState};
use crate::storage::{Db, ReadDb, SessionRepo};

/// Poll interval while sessions are active.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Messages to the watcher.
#[derive(Debug)]
pub enum WatchMsg {
    /// Watch this session until its row is closed (a launch handed off, or a session found
    /// open at start).
    Track(SessionId),
    /// Poll now (after a stop request).
    Poke,
    /// End the thread.
    Stop,
}

/// What one poll found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionPoll {
    /// Open rows, oldest first.
    pub open: Vec<Session>,
    /// Tracked sessions whose rows are now closed (reported once).
    pub ended: Vec<Session>,
}

/// The part of a row whose change is worth a UI update.
fn signature(s: &Session) -> (SessionId, SessionState, Option<u32>, bool) {
    (s.id, s.state, s.sandbox_pid, s.is_open())
}

/// The watcher's handle (owned by the AppService).
pub struct SessionWatcher {
    tx: Sender<WatchMsg>,
    thread: Option<JoinHandle<()>>,
}

impl SessionWatcher {
    /// Start the thread; each changed poll is passed to `report` (which posts an internal
    /// command to the AppService).
    pub fn spawn(
        db_file: PathBuf,
        report: impl Fn(SessionPoll) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (tx, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("session-watch".into())
            .spawn(move || run(&db_file, &rx, &report))?;
        Ok(Self {
            tx,
            thread: Some(thread),
        })
    }

    /// Send a message (ignored once the thread ended).
    pub fn send(&self, msg: WatchMsg) {
        let _ = self.tx.send(msg);
    }
}

impl Drop for SessionWatcher {
    fn drop(&mut self) {
        let _ = self.tx.send(WatchMsg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open_ro(db_file: &std::path::Path) -> Option<ReadDb> {
    match Db::open_ro(db_file) {
        Ok(db) => Some(db),
        Err(e) => {
            tracing::warn!(error = %e, "session watcher: cannot open the library");
            None
        }
    }
}

/// One poll: open rows and the rows of tracked sessions.
pub fn poll<S: SessionRepo + ?Sized>(
    db: &mut S,
    tracked: &mut BTreeSet<SessionId>,
) -> Option<SessionPoll> {
    let open = match db.open_sessions() {
        Ok(o) => o,
        Err(e) => {
            tracing::debug!(error = %e, "session watcher: poll failed");
            return None;
        }
    };
    let mut ended = Vec::new();
    for id in tracked.clone() {
        if open.iter().any(|s| s.id == id) {
            continue;
        }
        match db.session(id) {
            Ok(Some(s)) if !s.is_open() => {
                tracked.remove(&id);
                ended.push(s);
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                // Never inserted (start failed before the row): stop tracking.
                tracked.remove(&id);
            }
            Err(e) => tracing::debug!(error = %e, "session watcher: row read failed"),
        }
    }
    Some(SessionPoll { open, ended })
}

fn run(db_file: &std::path::Path, rx: &Receiver<WatchMsg>, report: &dyn Fn(SessionPoll)) {
    let mut db: Option<ReadDb> = None;
    let mut tracked: BTreeSet<SessionId> = BTreeSet::new();
    let mut last: Vec<(SessionId, SessionState, Option<u32>, bool)> = Vec::new();
    let mut active = false;
    loop {
        let msg = if active {
            match rx.recv_timeout(POLL_INTERVAL) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            // Parked: no timer while nothing runs.
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        };
        match msg {
            Some(WatchMsg::Stop) => return,
            Some(WatchMsg::Track(id)) => {
                tracked.insert(id);
            }
            Some(WatchMsg::Poke) | None => {}
        }
        if db.is_none() {
            db = open_ro(db_file);
        }
        let Some(conn) = db.as_mut() else {
            active = !tracked.is_empty();
            continue;
        };
        let Some(p) = poll(conn, &mut tracked) else {
            db = None;
            active = !tracked.is_empty();
            continue;
        };
        let sig: Vec<_> = p.open.iter().map(signature).collect();
        if sig != last || !p.ended.is_empty() {
            last = sig;
            report(p.clone());
        }
        active = !tracked.is_empty() || !p.open.is_empty();
    }
}
