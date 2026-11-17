//! The end of a runtime job: show its notices, rescan (SVC-008).
use crate::service::prelude::*;

/// The result of a runtime job (provision, verify, remove, background audit).
#[derive(Debug, Default)]
pub(crate) struct RuntimeJobDone {
    /// Warnings and errors to show (`EP-RT-020/021`, failures).
    pub notices: Vec<Error>,
}

impl AppService {
    pub(crate) fn on_runtime_job(&mut self, done: RuntimeJobDone) -> Result<()> {
        for n in done.notices {
            self.fail(n);
        }
        self.scan_runtimes();
        Ok(())
    }
}
