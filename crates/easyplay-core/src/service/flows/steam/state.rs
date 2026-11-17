//! What the Steam flow keeps between messages and what its jobs send back (ADR-0031).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::clock::{MonoInstant, Timestamp};
use crate::procfs::ProcIdentity;
use crate::service::prelude::*;
use crate::steam::client::SteamRequest;
use crate::steam::launch_options::Written;
use crate::steam::{Handoff, SteamScan};

/// How often the running state is read while a Steam game is starting or running.
pub const STEAM_POLL: Duration = Duration::from_secs(3);
/// How long a Play request waits to see the game start before it is forgotten.
pub const STEAM_START_WINDOW: Duration = Duration::from_secs(180);

/// The Steam part of the service's state.
#[derive(Default)]
pub(crate) struct SteamState {
    /// Starts the Steam client; `None` = requests to Steam are unavailable.
    pub handoff: Option<Arc<dyn Handoff>>,
    /// The option as last acted on.
    pub(super) enabled: bool,
    /// A scan job runs.
    pub(super) scanning: bool,
    /// Another scan was asked for while one ran (`Some(announce)`).
    pub(super) again: Option<bool>,
    /// Play requests not yet seen running: the end of their window.
    pub(super) pending: BTreeMap<u32, MonoInstant>,
    /// Games seen running, since.
    pub(super) running: BTreeMap<u32, Timestamp>,
    /// The polling thread's keep-running flag.
    pub(super) watcher: Option<Arc<AtomicBool>>,
}

impl SteamState {
    /// Whether Steam runs the game now (as last seen).
    pub fn is_running(&self, app_id: u32) -> bool {
        self.running.contains_key(&app_id)
    }
}

impl Drop for SteamState {
    fn drop(&mut self) {
        if let Some(f) = &self.watcher {
            f.store(false, Ordering::Relaxed);
        }
    }
}

/// Completions of the Steam jobs.
pub(crate) enum SteamDone {
    /// A scan finished; `announce` = asked for by the user (a toast follows).
    Scanned {
        scan: Box<SteamScan>,
        announce: bool,
    },
    /// Steam games running now.
    Running(BTreeMap<u32, ProcIdentity>),
    /// A request was handed to Steam (or not).
    Asked {
        game: Option<GameId>,
        request: SteamRequest,
        result: Result<()>,
    },
    /// The launch options were written (or not); the page's answer goes to `reply_to`.
    LaunchOptions {
        game: GameId,
        reply_to: QueryId,
        result: Result<Written>,
    },
}
