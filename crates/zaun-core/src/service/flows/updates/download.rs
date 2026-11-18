//! "Download": fetch, verify and stage the newest version of an owned install on the job
//! pool (ADR-0040 §5). Installing waits for a restart.

use super::UpdatesDone;
use super::publish::running;
use crate::service::prelude::*;
use crate::update::{UpdateError, check::fetch_release, stage};

impl AppService {
    pub(crate) fn download_update(&mut self) -> Result<()> {
        self.ensure_update_consent()?;
        let u = self.update_deps()?;
        let layout = u.install.layout().ok_or_else(|| err!(UpdManaged))?;
        if self.flow.updates.downloading {
            return Ok(());
        }
        let (check, files, keys, arch) = (
            Arc::clone(&u.check),
            Arc::clone(&u.files),
            u.keys.clone(),
            u.arch.clone(),
        );
        let (fs, post) = (Arc::clone(&self.deps.fs), self.poster());
        self.flow.updates.downloading = true;
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("update-download"),
            move |_| {
                let result = (|| -> Result<String, UpdateError> {
                    let release = fetch_release(&check, &keys)?;
                    release.ensure_newer(&running())?;
                    stage::download(&files, &*fs, &layout, &release, &arch)?;
                    Ok(release.version.to_string())
                })();
                post(Internal::Flow(Box::new(FlowDone::Updates(
                    UpdatesDone::Downloaded(result),
                ))));
            },
        );
        self.publish_updates();
        Ok(())
    }

    pub(super) fn on_downloaded(&mut self, result: Result<String, UpdateError>) -> Result<()> {
        self.flow.updates.downloading = false;
        let r = match result {
            Ok(v) => {
                tracing::info!(version = %v, "update staged");
                self.flow.updates.staged = Some(v);
                self.flow.updates.failure = None;
                Ok(())
            }
            Err(e) => {
                self.flow.updates.failure = Some(e.code());
                Err(e.into())
            }
        };
        self.publish_updates();
        r
    }
}
