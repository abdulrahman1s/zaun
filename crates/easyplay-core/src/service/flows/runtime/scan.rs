//! `RefreshRuntimes`: rescan the runtime folders on the pool (SVC-008).
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn scan_runtimes(&self) {
        let runtimes = Arc::clone(&self.deps.runtimes);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("runtime-refresh"),
            move |_| {
                // `refresh` scans on another worker; the pool has at least two.
                if let Ok(c) = runtimes.refresh().wait() {
                    post(Internal::Runtimes(c));
                }
            },
        );
    }
}
