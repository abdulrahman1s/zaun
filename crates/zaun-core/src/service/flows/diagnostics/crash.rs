//! The crash banner: load the record left by the last run, dismiss it (ERR-005).
use crate::diagnostics::crash;
use crate::service::prelude::*;

impl AppService {
    /// Load the crash banner (at start).
    pub(crate) fn load_crash(&mut self) {
        let dir = self.deps.paths.crash_dir.clone();
        let rec = crash::pending(&*self.deps.fs, &dir).map(|(_, r)| r);
        if rec.is_some() {
            self.update_diagnostics(|d| d.crash = rec);
        }
    }

    pub(crate) fn dismiss_crash(&mut self) -> Result<()> {
        if let Err(e) = crash::acknowledge(&*self.deps.fs, &self.deps.paths.crash_dir) {
            tracing::warn!("crash record not acknowledged: {e}");
        }
        self.update_diagnostics(|d| d.crash = None);
        Ok(())
    }
}
