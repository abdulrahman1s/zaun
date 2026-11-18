//! "Restart now": when no game or tool runs, leave a note for the process and close the
//! window; the process then installs the staged version and starts it (ADR-0040 §5).

use crate::service::prelude::*;

impl AppService {
    pub(crate) fn restart_for_update(&mut self) -> Result<()> {
        let u = self.update_deps()?;
        let layout = u.install.layout().ok_or_else(|| err!(UpdManaged))?;
        ensure!(
            self.flow.updates.staged.is_some(),
            UpdInstallFailed,
            detail = "nothing is downloaded"
        );
        ensure!(
            self.sessions.is_empty() && self.launching.is_empty(),
            UpdGameRunning
        );
        self.deps
            .fs
            .atomic_write(&layout.restart_file(), b"", 0o644)?;
        tracing::info!("restarting for the update");
        self.emit(Event::QuitRequested);
        Ok(())
    }
}
