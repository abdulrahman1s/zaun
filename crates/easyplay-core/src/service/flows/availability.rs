//! Whether each game's main program is still there (LOGIC_SPECS §9): one `stat` per game on
//! the job pool, re-entering as `FlowDone::Availability`. Runs at start, after a game is
//! added or its program located, and on `RescanMissing` ("Check again", Refresh). Until it
//! answers a game is `Unknown`, which no collection counts as installed or missing.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::FlowDone;
use crate::fsx::MountTable;
use crate::jobs::{JobLabel, JobPriority};
use crate::library::missing::{self, MOUNT_DEADLINE};

impl AppService {
    /// Check every game of the library.
    pub(crate) fn check_availability(&mut self) {
        let snapshot = self.lib.snapshot();
        let targets = missing::targets_from_snapshot(&snapshot, &BTreeMap::new());
        if targets.is_empty() {
            return;
        }
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let media = missing::media_roots(&self.deps.launch_env.user_name);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("availability"),
            move |_cancel| {
                let mounts = MountTable::read(&*fs).unwrap_or_default();
                let found = missing::scan(fs, &*clock, &mounts, &media, &targets, MOUNT_DEADLINE);
                post(Internal::Flow(Box::new(FlowDone::Availability(found))));
            },
        );
    }
}
