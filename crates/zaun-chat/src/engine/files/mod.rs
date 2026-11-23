//! Files over XFTP (Phase 2, groups in Phase 4): sending, fetching, cancelling, opening and locating a message's file, and the friend's or member's file offers (Commands.hs `APISendMessages`, `CancelFile`; Subscriber.hs `processFileInvitation`, `processFDMessage`, `xFileCancel`).

mod body;
mod cancel;
mod config;
mod descr;
mod done;
mod forget;
mod inbound;
mod locate;
mod offer;
mod real;
mod receive;
mod send;
mod state;
pub(crate) mod transfers;
mod uploaded;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub(crate) use body::edit_caption;
pub(crate) use inbound::offer;
pub use transfers::{FileDone, FileKey, Transfers};

use crate::xftp::XftpServer;

/// Transfers started at once; more are refused, so the queue behind the two threads is bounded.
pub const MAX_ACTIVE: usize = 16;

/// A transfer in flight: its run number and the flag that stops it.
#[derive(Debug)]
pub struct Active {
    pub job: u64,
    pub cancel: Arc<AtomicBool>,
}

/// What the engine keeps about files: where they live, the servers, and the transfers in flight.
pub struct Files {
    /// The engine's data folder (Hello `dir`).
    pub dir: PathBuf,
    /// The servers an upload goes to, and the ones a download is approved from.
    pub servers: Vec<XftpServer>,
    pub transfers: Box<dyn Transfers>,
    /// Eviction: an entry leaves when its job's result arrives or the transfer is cancelled.
    pub active: HashMap<FileKey, Active>,
    next_job: u64,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            servers: Vec::new(),
            transfers: Box::new(transfers::NoTransfers),
            active: HashMap::new(),
            next_job: 0,
        }
    }
}

impl Files {
    /// Records a new run of `key`'s transfer; the run number and its cancel flag.
    pub fn start(&mut self, key: FileKey) -> (u64, Arc<AtomicBool>) {
        self.next_job += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let job = self.next_job;
        let active = Active {
            job,
            cancel: Arc::clone(&cancel),
        };
        self.active.insert(key, active);
        (job, cancel)
    }

    /// Takes the transfer out when `job` is still the run wanted; a result of an older run is not.
    pub fn finish(&mut self, key: FileKey, job: u64) -> bool {
        if self.active.get(&key).is_some_and(|a| a.job == job) {
            self.active.remove(&key);
            return true;
        }
        false
    }

    #[must_use]
    pub fn is_current(&self, key: FileKey, job: u64) -> bool {
        self.active.get(&key).is_some_and(|a| a.job == job)
    }

    /// Whether `server` is one the user's list holds, passwords aside (`getUnknownSrvs`).
    #[must_use]
    pub fn knows(&self, server: &XftpServer) -> bool {
        let bare = server.without_auth();
        self.servers.iter().any(|k| k.without_auth() == bare)
    }
}

impl std::fmt::Debug for Files {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Files")
            .field("active", &self.active.len())
            .finish_non_exhaustive()
    }
}
