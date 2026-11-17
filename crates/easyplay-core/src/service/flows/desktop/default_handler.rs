//! `SetDefaultHandler`: take over or give back the `.exe` handler, only on the user's request (DESKTOP_INTEGRATION §4).
use super::DesktopDone;
use crate::config::PreferencePatch;
use crate::desktop::mime::{KV_PREVIOUS, PreviousHandlers};
use crate::service::prelude::*;
use crate::storage::KvRepo;

impl AppService {
    pub(crate) fn set_default_handler(&mut self, enable: bool) -> Result<()> {
        let choice = if enable { "yes" } else { "no" };
        self.handle(Command::SetPreference(
            PreferencePatch::new().set("integration.handle_exe", choice),
        ));
        if enable {
            return self.desktop_job("desktop-handler", |d| {
                let types = d.mime.managed_types();
                DesktopDone::HandlerSet(d.mime.set_default(&types))
            });
        }
        let previous = match self.lib.db_mut().kv_get(KV_PREVIOUS) {
            Ok(Some(json)) => serde_json::from_str::<PreviousHandlers>(&json).ok(),
            _ => None,
        };
        let Some(previous) = previous else {
            // Nothing was taken over; "no" only records the choice.
            self.refresh_integration();
            return Ok(());
        };
        self.desktop_job("desktop-handler", move |d| {
            DesktopDone::HandlerRestored(d.mime.restore(&previous))
        })
    }
}
