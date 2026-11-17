//! Desktop jobs and their results: run on the pool, apply on the service thread (DESKTOP_INTEGRATION §4, §6, §8).
use crate::desktop::mime::{KV_PREVIOUS, PreviousHandlers};
use crate::desktop::{DesktopServices, IntegrationState};
use crate::service::prelude::*;
use crate::storage::KvRepo;

/// Results of desktop jobs.
pub(crate) enum DesktopDone {
    /// A fresh integration state.
    State(IntegrationState),
    /// EasyPlay became the default; the previous handlers.
    HandlerSet(Result<PreviousHandlers>),
    /// The previous handlers were restored.
    HandlerRestored(Result<()>),
    /// A job failed (shown as a failure).
    Failed(Error),
    /// Done; nothing to publish.
    Done,
}

impl AppService {
    pub(super) fn desktop(&self) -> Option<Arc<DesktopServices>> {
        self.deps.desktop.clone()
    }

    pub(super) fn desktop_job(
        &self,
        label: &'static str,
        f: impl FnOnce(&DesktopServices) -> DesktopDone + Send + 'static,
    ) -> Result<()> {
        let Some(d) = self.desktop() else {
            return Err(Error::not_available("DESKTOP-005"));
        };
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel(label), move |_| {
                post(Internal::Desktop(Box::new(f(&d))));
            });
        Ok(())
    }

    pub(crate) fn on_desktop(&mut self, done: DesktopDone) -> Result<()> {
        match done {
            DesktopDone::Done => {}
            DesktopDone::State(mut s) => {
                let cur = self.hub.integration.load();
                if s.handler.is_none() {
                    s.handler = cur.handler.clone();
                    s.handler_unavailable = cur.handler_unavailable;
                }
                self.hub.integration.store(Arc::new(s));
                self.emit(Event::HostFactsChanged);
                // Shortcuts of games renamed or removed while EasyPlay was closed.
                self.sync_shortcuts();
            }
            DesktopDone::HandlerSet(Ok(previous)) => {
                let now = self.deps.clock.now();
                let db = self.lib.db_mut();
                // Only the first takeover is recorded: later ones would record EasyPlay.
                if db.kv_get(KV_PREVIOUS).ok().flatten().is_none()
                    && let Ok(json) = serde_json::to_string(&previous)
                    && let Err(e) = db.kv_set(KV_PREVIOUS, &json, now)
                {
                    tracing::warn!(error = %e, "previous handlers not recorded");
                }
                self.refresh_integration();
            }
            DesktopDone::HandlerRestored(Ok(())) => {
                // A leftover record is harmless: the next takeover only skips recording.
                let _ = self.lib.db_mut().kv_delete(KV_PREVIOUS);
                self.refresh_integration();
            }
            DesktopDone::HandlerSet(Err(e)) | DesktopDone::HandlerRestored(Err(e)) => {
                self.refresh_integration();
                return Err(e);
            }
            DesktopDone::Failed(e) => return Err(e),
        }
        Ok(())
    }
}
