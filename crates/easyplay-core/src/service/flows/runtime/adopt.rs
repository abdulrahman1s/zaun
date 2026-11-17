//! `AdoptRuntime`: take over an SLR variant umu already installed for the user (SVC-008, `runtime::adopt`).
use crate::model::SlrVariant;
use crate::runtime::{AdoptReport, RuntimeStore, adopt_slr};
use crate::service::prelude::*;
use crate::storage::RuntimeRepo;

impl AppService {
    pub(crate) fn adopt_runtime(&mut self, variant: SlrVariant) -> Result<()> {
        let catalog = self.hub.runtimes.load();
        let source = catalog
            .foreign
            .slr
            .iter()
            .find(|s| s.variant == variant && s.is_complete())
            .map(|s| s.path.clone())
            .ok_or_else(|| {
                err!(
                    RtInvalidFolder,
                    variant = variant.as_str(),
                    rule = "no complete install of this runtime outside the store"
                )
            })?;
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
        Ok(())
    }

    pub(crate) fn on_adopted(&mut self, r: Result<AdoptReport>) -> Result<()> {
        let report = r?;
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
        Ok(())
    }
}
