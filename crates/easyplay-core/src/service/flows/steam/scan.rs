//! The option and the scans of Steam's files (ADR-0031).

use super::state::SteamDone;
use crate::model::Activity;
use crate::service::prelude::*;
use crate::steam::client::running_games;
use crate::steam::{Handoff, SteamScan};

impl AppService {
    /// Use `handoff` to reach the Steam client (tests, the headless command line). The GUI's
    /// service uses `session::RealHandoff` whenever desktop integration is on.
    pub fn set_steam_handoff(&mut self, handoff: Arc<dyn Handoff>) {
        self.flow.steam.handoff = Some(handoff);
    }

    /// Start-up: scan when the option is on.
    pub(crate) fn steam_start(&mut self) {
        if self.flow.steam.handoff.is_none() && self.deps.desktop.is_some() {
            self.flow.steam.handoff = Some(Arc::new(crate::session::RealHandoff));
        }
        self.steam_preference_changed();
    }

    /// The preferences changed: scan when the option was turned on, clear the Steam games
    /// when it was turned off. Failures are shown here: the caller is not a handler.
    pub(crate) fn steam_preference_changed(&mut self) {
        let on = self.prefs.steam.show_games;
        if on == self.flow.steam.enabled {
            return;
        }
        self.flow.steam.enabled = on;
        if on {
            self.steam_scan(false);
        } else if let Err(e) = self.steam_clear() {
            self.fail(e);
        }
    }

    fn steam_clear(&mut self) -> Result<()> {
        let st = &mut self.flow.steam;
        let idle: Vec<(GameId, Activity)> = st
            .pending
            .keys()
            .chain(st.running.keys())
            .map(|a| (GameId::steam(*a), Activity::Idle))
            .collect();
        st.pending.clear();
        st.running.clear();
        self.lib.steam_set_running(&idle);
        self.steam_stop_watch();
        self.lib.set_steam(None)?;
        self.publish_library();
        Ok(())
    }

    /// Read Steam's files on the job pool (`announce`: a toast with the count follows).
    pub(crate) fn steam_scan(&mut self, announce: bool) {
        if !self.flow.steam.enabled {
            return;
        }
        if self.flow.steam.scanning {
            let a = self.flow.steam.again.unwrap_or(false) || announce;
            self.flow.steam.again = Some(a);
            return;
        }
        self.flow.steam.scanning = true;
        let fs = Arc::clone(&self.deps.fs);
        let home = self.deps.launch_env.home.clone();
        let proc = Arc::clone(&self.deps.proc);
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Background, JobLabel("steam-scan"), move |_| {
                let scan = crate::steam::scan(&*fs, &home);
                let running = running_games(&*proc);
                let scanned = SteamDone::Scanned {
                    scan: Box::new(scan),
                    announce,
                };
                post(Internal::Flow(Box::new(FlowDone::Steam(scanned))));
                post(Internal::Flow(Box::new(FlowDone::Steam(
                    SteamDone::Running(running),
                ))));
            });
    }

    pub(super) fn on_steam_scanned(&mut self, scan: SteamScan, announce: bool) -> Result<()> {
        self.flow.steam.scanning = false;
        if let Some(a) = self.flow.steam.again.take() {
            // The files may have changed meanwhile: read them again before publishing.
            self.steam_scan(a || announce);
            return Ok(());
        }
        if !self.flow.steam.enabled {
            return Ok(());
        }
        if announce && scan.roots.is_empty() {
            self.fail(err!(StmNotFound));
        }
        let count = scan.games.len();
        let found = !scan.roots.is_empty();
        tracing::info!(
            roots = scan.roots.len(),
            games = count,
            skipped = scan.skipped.len(),
            "steam scan"
        );
        self.lib.set_steam(Some(Arc::new(scan)))?;
        self.steam_index_art();
        self.publish_library();
        if announce && found {
            self.notice(Notice::SteamScanned { games: count });
        }
        Ok(())
    }

    /// Point the artwork index at the pictures in Steam's cache (after a scan and after the
    /// index was rebuilt from the database rows).
    pub(crate) fn steam_index_art(&self) {
        let Some(scan) = self.lib.steam_scan() else {
            return;
        };
        for g in &scan.games {
            for a in &g.art {
                self.deps
                    .art_index
                    .insert_path(a.hash, a.path.clone(), a.slot);
            }
        }
    }
}
