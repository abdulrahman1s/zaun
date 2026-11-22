//! "Check now" and the daily check: fetch and verify the newest release on the job pool,
//! remember it, announce a newer version once.

use super::UpdatesDone;
use super::publish::running;
use crate::service::prelude::*;
use crate::update::check::fetch_release;
use crate::update::source::CHECK_EVERY;
use crate::update::{Release, UpdateError, state::Latest};

impl AppService {
    /// "Check now" (`manual`) refuses without consent; the daily check just does nothing.
    pub(crate) fn check_updates(&mut self, manual: bool) {
        let ready = self
            .ensure_update_consent()
            .and_then(|()| self.update_deps().map(|_| ()));
        if let Err(e) = ready {
            if manual {
                self.fail(e);
            }
            return;
        }
        if std::mem::replace(&mut self.flow.updates.checking, true) {
            return;
        }
        let Some(u) = &self.deps.updates else { return };
        let (http, keys, post) = (Arc::clone(&u.check), u.keys.clone(), self.poster());
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("update-check"),
            move |_| {
                let result = fetch_release(&http, &keys).map(Box::new);
                post(Internal::Flow(Box::new(FlowDone::Updates(
                    UpdatesDone::Checked { manual, result },
                ))));
            },
        );
        self.publish_updates();
    }

    pub(super) fn on_checked(
        &mut self,
        manual: bool,
        result: Result<Box<Release>, UpdateError>,
    ) -> Result<()> {
        self.flow.updates.checking = false;
        match result {
            Ok(r) => {
                self.remember_release(&r);
                self.download_in_background(&r);
            }
            Err(e) => {
                tracing::info!(manual, error = %e, "update check failed");
                self.flow.updates.failure = Some(e.code());
            }
        }
        self.publish_updates();
        // After a daily check, the next one a day from now. "Check now" leaves the pending
        // timer alone: when it runs out, `schedule_updates` waits for the rest of the day.
        if !manual && self.prefs.updates.check {
            self.flow.updates.generation += 1;
            self.updates_after(CHECK_EVERY);
        }
        Ok(())
    }

    /// A copy that updates itself fetches a newer version at once, as Discord and Steam do;
    /// it is switched to at the next start. Never a version that already failed to start.
    fn download_in_background(&mut self, r: &Release) {
        let u = &self.flow.updates;
        let version = r.version.to_string();
        let wanted = r.version > running()
            && !u.downloading
            && u.staged.as_deref() != Some(version.as_str())
            && u.file.rolled_back.as_deref() != Some(version.as_str())
            && self
                .deps
                .updates
                .as_ref()
                .is_some_and(|d| d.install.channel.self_updates());
        if wanted && let Err(e) = self.download_update() {
            tracing::info!(error = %e, "update not downloaded");
        }
    }

    fn remember_release(&mut self, r: &Release) {
        let newer = r.version > running();
        let u = &mut self.flow.updates;
        u.failure = None;
        u.file.last_checked = Some(self.deps.clock.now());
        u.file.latest = newer.then(|| Latest::of(r));
        let version = r.version.to_string();
        let announce = newer && u.file.announced.as_deref() != Some(version.as_str());
        if announce {
            u.file.announced = Some(version.clone());
        }
        self.save_update_state();
        if announce {
            self.notice(Notice::UpdateAvailable { version });
        }
    }
}
