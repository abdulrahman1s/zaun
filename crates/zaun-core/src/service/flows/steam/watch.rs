//! Which Steam games run, seen through `/proc` by a polling thread (ADR-0031).
use crate::service::prelude::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use super::state::{STEAM_POLL, SteamDone};
use crate::procfs::ProcIdentity;
use crate::steam::client::running_games;

impl AppService {
    pub(super) fn on_steam_running(&mut self, now_running: &BTreeMap<u32, ProcIdentity>) {
        if !self.flow.steam.enabled {
            // A poll that finished after the option was turned off.
            self.steam_stop_watch();
            return;
        }
        let changes = self.steam_running_changes(now_running);
        let st = &self.flow.steam;
        let idle = st.pending.is_empty() && st.running.is_empty();
        if !changes.is_empty() {
            self.lib.steam_set_running(&changes);
            self.publish_library();
        }
        if idle {
            self.steam_stop_watch();
        } else {
            self.steam_watch();
        }
    }

    /// Fold one poll into the state: started, ended and expired games, as activity changes.
    fn steam_running_changes(
        &mut self,
        now_running: &BTreeMap<u32, ProcIdentity>,
    ) -> Vec<(GameId, Activity)> {
        let now = self.deps.clock.now();
        let mono = self.deps.clock.mono();
        let st = &mut self.flow.steam;
        let mut changes: Vec<(GameId, Activity)> = Vec::new();
        for app in now_running.keys() {
            st.pending.remove(app);
            if !st.running.contains_key(app) {
                st.running.insert(*app, now);
                changes.push((GameId::steam(*app), Activity::Running { since: now }));
            }
        }
        let ended: Vec<u32> = st
            .running
            .keys()
            .filter(|a| !now_running.contains_key(a))
            .copied()
            .collect();
        for app in ended {
            st.running.remove(&app);
            changes.push((GameId::steam(app), Activity::Idle));
        }
        let expired: Vec<u32> = st
            .pending
            .iter()
            .filter(|(_, until)| **until <= mono)
            .map(|(a, _)| *a)
            .collect();
        for app in expired {
            st.pending.remove(&app);
            changes.push((GameId::steam(app), Activity::Idle));
        }
        changes
    }

    /// Poll the running state now (tests and the headless command line; the GUI's service
    /// polls on its own thread).
    pub fn steam_poll_now(&mut self) {
        let r = running_games(&*self.deps.proc);
        self.on_steam_running(&r);
    }

    pub(super) fn steam_watch(&mut self) {
        let running = self
            .flow
            .steam
            .watcher
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed));
        if !self.deps.watch_sessions || running {
            return;
        }
        let flag = Arc::new(AtomicBool::new(true));
        let keep = Arc::clone(&flag);
        let proc = Arc::clone(&self.deps.proc);
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("steam-watch".into())
            .spawn(move || {
                while keep.load(Ordering::Relaxed) {
                    std::thread::sleep(STEAM_POLL);
                    if !keep.load(Ordering::Relaxed) {
                        break;
                    }
                    let r = running_games(&*proc);
                    post(Internal::Flow(Box::new(FlowDone::Steam(
                        SteamDone::Running(r),
                    ))));
                }
            });
        match spawned {
            Ok(_) => self.flow.steam.watcher = Some(flag),
            Err(e) => tracing::warn!(error = %e, "steam watcher not started"),
        }
    }

    pub(super) fn steam_stop_watch(&mut self) {
        if let Some(f) = self.flow.steam.watcher.take() {
            f.store(false, Ordering::Relaxed);
        }
    }
}
