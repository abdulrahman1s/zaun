//! When checks happen: at start when a day has passed, then every day while Zaun runs, and
//! never while `updates.check` is off (ADR-0040 §4).

use super::UpdatesDone;
use super::publish::running;
use crate::service::prelude::*;
use crate::update::source::CHECK_EVERY;
use crate::update::{apply, state};

impl AppService {
    pub(crate) fn updates_start(&mut self) {
        let path = state::state_file(&self.deps.paths);
        self.flow.updates.file = crate::update::UpdateState::load(&*self.deps.fs, &path);
        self.flow.updates.enabled = self.prefs.updates.check;
        if let Some(u) = &self.deps.updates
            && let Some(layout) = u.install.layout()
        {
            self.flow.updates.staged = match apply::staged_release(&*self.deps.fs, &layout, &u.keys)
            {
                Ok(r) => r
                    .filter(|r| r.version > running())
                    .map(|r| r.version.to_string()),
                Err(e) => {
                    tracing::warn!(error = %e, "the staged update is not valid");
                    None
                }
            };
        }
        self.publish_updates();
        self.schedule_updates();
    }

    pub(crate) fn updates_preference_changed(&mut self) {
        if self.prefs.updates.check != self.flow.updates.enabled {
            self.flow.updates.enabled = self.prefs.updates.check;
            self.schedule_updates();
            self.publish_updates();
        }
    }

    /// Check now if a day has passed, otherwise wait for the rest of the day. Older timers
    /// are cancelled by the new generation.
    pub(super) fn schedule_updates(&mut self) {
        self.flow.updates.generation += 1;
        if !self.prefs.updates.check || self.update_deps().is_err() {
            return;
        }
        let now = self.deps.clock.now();
        let file = &self.flow.updates.file;
        if file.due(now) {
            self.check_updates(false);
            return;
        }
        let since = file
            .last_checked
            .and_then(|t| now.since(t))
            .unwrap_or_default();
        self.updates_after(CHECK_EVERY.saturating_sub(since));
    }

    /// Post `Due` after `wait`, from a thread of its own.
    pub(super) fn updates_after(&self, wait: Duration) {
        let generation = self.flow.updates.generation;
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("update-timer".into())
            .spawn(move || {
                std::thread::sleep(wait);
                post(Internal::Flow(Box::new(FlowDone::Updates(
                    UpdatesDone::Due { generation },
                ))));
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "updates: timer not started");
        }
    }
}
