//! Runtime commands (SVC-008): rescans, and adopting an SLR variant umu already installed
//! for the user (`runtime::adopt`). Provision, removal and verification arrive with M7.

use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::super::event::Notice;
use crate::error::{Error, ErrorCode};
use crate::jobs::{JobLabel, JobPriority};
use crate::model::SlrVariant;
use crate::runtime::{AdoptReport, RuntimeStore, adopt_slr};
use crate::storage::RuntimeRepo;

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

    pub(crate) fn adopt_runtime(&mut self, variant: SlrVariant) {
        let catalog = self.hub.runtimes.load();
        let Some(source) = catalog
            .foreign
            .slr
            .iter()
            .find(|s| s.variant == variant && s.is_complete())
            .map(|s| s.path.clone())
        else {
            return self.fail(
                Error::new(ErrorCode::RtInvalidFolder)
                    .with("variant", variant.as_str())
                    .with(
                        "rule",
                        "no complete install of this runtime outside the store",
                    ),
            );
        };
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let store = RuntimeStore::new(&self.deps.paths);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("runtime-adopt"),
            move |cancel| {
                let now = clock.now();
                let r = adopt_slr(&*fs, &*clock, &store, &source, variant, now, cancel);
                post(Internal::Adopted(r));
            },
        );
    }

    pub(crate) fn on_adopted(&mut self, r: Result<AdoptReport, Error>) {
        let report = match r {
            Ok(r) => r,
            Err(e) => return self.fail(e),
        };
        if let Some(install) = &report.install {
            let now = self.deps.clock.now();
            let db = self.lib.db_mut();
            if let Err(e) = db
                .put_slr_install(install)
                .and_then(|()| db.record_audit_result(install.variant, now, "clean"))
            {
                tracing::warn!(error = %e, "runtime install not recorded");
            }
        }
        self.notice(Notice::RuntimeAdopted {
            variant: report.variant,
            reflinked: report.files > 0 && report.reflinked == report.files,
        });
        self.scan_runtimes();
    }
}
