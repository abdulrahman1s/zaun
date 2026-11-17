//! The integration state (handler, shortcuts) and keeping shortcuts in step with the library (DESKTOP_INTEGRATION §4, §6).
use crate::desktop::mime;
use crate::desktop::{DesktopServices, IntegrationState};
use crate::service::prelude::*;

use super::DesktopDone;

impl AppService {
    /// Query the handler and list the shortcuts (start-up and after every change).
    pub(crate) fn refresh_integration(&self) {
        if self.desktop().is_none() {
            return;
        }
        // Cannot fail: the desktop services were just seen.
        let _ = self.desktop_job("desktop-status", |d| {
            DesktopDone::State(integration_state(d))
        });
    }

    /// Keep shortcuts in step with the library: renamed games are rewritten, removed games
    /// lose theirs (§6). Cheap: one directory listing on the job pool.
    pub(crate) fn sync_shortcuts(&self) {
        let current = self.hub.integration.load();
        let snap = self.lib.snapshot();
        let mut work: Vec<(GameId, Option<String>)> = Vec::new();
        for s in &current.shortcuts {
            match snap.get(s.id) {
                None => work.push((s.id, None)),
                Some(g)
                    if crate::desktop::desktop_entry::escape_value(&g.name)
                        != crate::desktop::desktop_entry::escape_value(&s.name) =>
                {
                    work.push((s.id, Some(g.name.to_string())));
                }
                Some(_) => {}
            }
        }
        if work.is_empty() {
            return;
        }
        let queued = self.desktop_job("desktop-shortcut-sync", move |d| {
            for (id, name) in work {
                let r = match name {
                    Some(n) => d.shortcuts.rename(id, &n),
                    None => d.shortcuts.remove(id),
                };
                if let Err(e) = r {
                    tracing::warn!(game = %id, error = %e, "shortcut not updated");
                }
            }
            DesktopDone::State(integration_state_shortcuts(d))
        });
        if let Err(e) = queued {
            self.fail(e);
        }
    }
}

pub(super) fn integration_state_shortcuts(d: &DesktopServices) -> IntegrationState {
    IntegrationState {
        handler: None,
        handler_unavailable: false,
        shortcuts: d.shortcuts.list(),
    }
}

fn integration_state(d: &DesktopServices) -> IntegrationState {
    let (handler, unavailable) = if d.mime_available {
        match d.mime.status() {
            Ok(s) => (Some(mime::overall(&s)), false),
            Err(e) => {
                tracing::info!(error = %e, "handler status unknown");
                (None, false)
            }
        }
    } else {
        (None, true)
    };
    IntegrationState {
        handler,
        handler_unavailable: unavailable,
        shortcuts: d.shortcuts.list(),
    }
}
